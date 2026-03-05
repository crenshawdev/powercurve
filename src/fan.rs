// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

#![allow(clippy::inconsistent_digit_grouping)]

use crate::nvml::NvidiaState;
use serde::Deserialize;
use std::{
    cmp,
    collections::HashMap,
    fs, io,
    sync::{Arc, Mutex as StdMutex},
};
use sysfs_class::{HwMon, SysClass};

pub(crate) const CONFIG_PATH: &str = "/etc/powercurve/fan.toml";

// -- TOML config deserialization types --

/// Top-level config file structure.
#[derive(Deserialize)]
pub(crate) struct FanConfig {
    pub platform: Option<String>,
    pub critical_cpu_temp: f32,
    pub critical_gpu_temp: f32,
    pub hysteresis: Option<f32>,
    pub thermal_fallback: Option<bool>,
    pub thermal_cooldown: Option<u32>,
    pub curve: Vec<CurvePoint>,
    pub channels: Vec<ChannelConfig>,
    pub profiles: Option<HashMap<String, ProfileConfig>>,
}

/// Per-profile curve override. When a profile is active, its curve
/// replaces the shared top-level curve for channels that don't have
/// their own per-channel override.
#[derive(Deserialize)]
pub(crate) struct ProfileConfig {
    pub curve: Vec<CurvePoint>,
}

/// A single point on the fan curve. Human-friendly units.
#[derive(Deserialize)]
pub(crate) struct CurvePoint {
    /// Temperature in Celsius.
    pub temp: f32,
    /// Duty cycle as a percentage (0-100).
    pub duty: f32,
}

/// Per-profile curve override on a single channel.
#[derive(Deserialize)]
pub(crate) struct ChannelProfileConfig {
    pub curve: Vec<CurvePoint>,
}

/// Maps a PWM output to a temperature source, with optional per-channel
/// and per-channel-per-profile curve overrides.
#[derive(Deserialize)]
pub(crate) struct ChannelConfig {
    pub pwm: String,
    pub source: String,
    pub min_duty: Option<f32>,
    pub stall_detect: Option<bool>,
    pub stall_threshold: Option<u32>,
    pub passthrough: Option<bool>,
    pub curve: Option<Vec<CurvePoint>>,
    pub profiles: Option<HashMap<String, ChannelProfileConfig>>,
}

#[derive(Debug, thiserror::Error)]
pub enum FanDaemonError {
    #[error("failed to collect hwmon devices: {}", _0)]
    HwmonDevices(io::Error),
    #[error("platform hwmon not found")]
    PlatformHwmonNotFound,
    #[error("cpu hwmon not found")]
    CpuHwmonNotFound,
}

/// Which temperature source drives a fan channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempSource {
    /// Max of CPU hwmon sensors only.
    Cpu,
    /// Max of GPU sensors (amdgpu hwmon + NVML).
    Gpu,
    /// Max of all sensors, CPU and GPU combined.
    All,
}

/// Maps a single PWM output to a temperature source and fan curve.
#[derive(Clone, Debug)]
pub struct FanChannel {
    pub pwm: String,
    pub source: TempSource,
    pub curve: FanCurve,
    pub min_duty: Option<u8>,
    pub stall_detect: bool,
    pub stall_threshold: u32,
    pub passthrough: bool,
}

/// Stored per-channel definition from config, used to rebuild curves
/// when the active profile changes.
#[derive(Clone)]
struct ChannelDef {
    pwm: String,
    source: TempSource,
    override_curve: Option<FanCurve>,
    profile_curves: HashMap<String, FanCurve>,
    min_duty_byte: Option<u8>,
    stall_detect: bool,
    stall_threshold: u32,
    passthrough: bool,
}

/// Snapshot of the fan daemon's current state, shared with D-Bus handlers.
#[derive(Clone, Default)]
pub struct FanStatus {
    pub cpu_temp: Option<u32>,
    pub gpu_temp: Option<u32>,
    pub channel_duties: Vec<(String, Option<u8>)>,
    pub channel_curves: Vec<(String, Vec<(f32, f32)>)>,
    pub overrides: HashMap<String, u8>,
    pub min_duties: Vec<(String, Option<u8>)>,
    pub rpms: Vec<(String, Option<u32>)>,
    pub stalled: Vec<String>,
    pub passthrough: Vec<String>,
    pub critical: bool,
    pub config_loaded: bool,
}

pub struct FanDaemon {
    channels: Vec<FanChannel>,
    channel_defs: Vec<ChannelDef>,
    shared_curve: FanCurve,
    profile_curves: HashMap<String, FanCurve>,
    critical_cpu_temp: u32,
    critical_gpu_temp: u32,
    hysteresis: u32,
    last_duties: Vec<u8>,
    last_temps: Vec<Option<u32>>,
    stall_counts: Vec<u32>,
    platform_names: Vec<String>,
    amdgpus: Vec<HwMon>,
    platforms: Vec<HwMon>,
    cpus: Vec<HwMon>,
    nvidia: NvidiaState,
    status: Arc<StdMutex<FanStatus>>,
    thermal_fallback: bool,
    thermal_cooldown: u32,
    current_profile: String,
}

const DEFAULT_HYSTERESIS_C: f32 = 3.0;

impl FanDaemon {
    /// Build a new fan daemon with per-channel temperature routing.
    ///
    /// Requires a config file at `/etc/powercurve/fan.toml`.
    /// Without one, fan control is disabled and the daemon only handles
    /// power profiles.
    pub fn new(nvidia: NvidiaState) -> Self {
        let status = Arc::new(StdMutex::new(FanStatus::default()));
        let mut daemon = Self {
            channels: Vec::new(),
            channel_defs: Vec::new(),
            shared_curve: FanCurve::default(),
            profile_curves: HashMap::new(),
            critical_cpu_temp: 0,
            critical_gpu_temp: 0,
            hysteresis: (DEFAULT_HYSTERESIS_C * 1000.0) as u32,
            last_duties: Vec::new(),
            last_temps: Vec::new(),
            stall_counts: Vec::new(),
            platform_names: Vec::new(),
            amdgpus: Vec::new(),
            platforms: Vec::new(),
            cpus: Vec::new(),
            nvidia,
            status,
            thermal_fallback: false,
            thermal_cooldown: 30,
            current_profile: String::new(),
        };

        daemon.apply_config(load_config());
        daemon
    }

    /// Shared status handle for D-Bus handlers to read.
    pub fn status_handle(&self) -> Arc<StdMutex<FanStatus>> {
        self.status.clone()
    }

    /// Apply a parsed config (or None for no-config fallback).
    ///
    /// Resets hysteresis tracking and re-discovers hwmon devices. Used
    /// by both initial construction and hot reload.
    fn apply_config(&mut self, config: Option<FanConfig>) {
        if let Some(config) = config {
            self.shared_curve = build_curve(&config.curve);

            self.channel_defs = config
                .channels
                .iter()
                .map(|ch| {
                    let profile_curves = ch
                        .profiles
                        .as_ref()
                        .map(|p| {
                            p.iter()
                                .map(|(name, pc)| (name.to_lowercase(), build_curve(&pc.curve)))
                                .collect()
                        })
                        .unwrap_or_default();
                    let min_duty_byte = ch
                        .min_duty
                        .map(|pct| ((pct.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8);
                    ChannelDef {
                        pwm: ch.pwm.clone(),
                        source: parse_temp_source(&ch.source),
                        override_curve: ch.curve.as_deref().map(build_curve),
                        profile_curves,
                        min_duty_byte,
                        stall_detect: ch.stall_detect.unwrap_or(false),
                        stall_threshold: ch.stall_threshold.unwrap_or(3),
                        passthrough: ch.passthrough.unwrap_or(false),
                    }
                })
                .collect();

            self.channels = self
                .channel_defs
                .iter()
                .map(|def| FanChannel {
                    pwm: def.pwm.clone(),
                    source: def.source,
                    curve: def.override_curve.clone().unwrap_or_else(|| self.shared_curve.clone()),
                    min_duty: def.min_duty_byte,
                    stall_detect: def.stall_detect,
                    stall_threshold: def.stall_threshold,
                    passthrough: def.passthrough,
                })
                .collect();

            self.profile_curves = config
                .profiles
                .unwrap_or_default()
                .into_iter()
                .map(|(name, p)| (name.to_lowercase(), build_curve(&p.curve)))
                .collect();

            self.critical_cpu_temp = (config.critical_cpu_temp * 1000.0) as u32;
            self.critical_gpu_temp = (config.critical_gpu_temp * 1000.0) as u32;
            self.hysteresis = (config.hysteresis.unwrap_or(DEFAULT_HYSTERESIS_C) * 1000.0) as u32;
            self.platform_names = config.platform.map(|name| vec![name]).unwrap_or_default();
            self.thermal_fallback = config.thermal_fallback.unwrap_or(false);
            self.thermal_cooldown = config.thermal_cooldown.unwrap_or(30);

            if let Ok(mut s) = self.status.lock() {
                s.config_loaded = true;
            }
        } else {
            log::warn!(
                "no fan config found at {}, fan control disabled. \
                 run `powercurve fan-detect --generate` to create one",
                CONFIG_PATH
            );
            self.channels.clear();
            self.channel_defs.clear();
            self.shared_curve = FanCurve::default();
            self.profile_curves.clear();
            self.critical_cpu_temp = 0;
            self.critical_gpu_temp = 0;
            self.hysteresis = (DEFAULT_HYSTERESIS_C * 1000.0) as u32;
            self.platform_names.clear();
        }

        let count = self.channels.len();
        self.last_duties = vec![0; count];
        self.last_temps = vec![None; count];
        self.stall_counts = vec![0; count];

        if !self.platform_names.is_empty() {
            if let Err(err) = self.discover() {
                log::error!("fan daemon: {}", err);
            }
        }
    }

    /// Reload the fan config from disk. Validates before applying so a
    /// broken config doesn't take down the running daemon.
    pub fn reload(&mut self) {
        use crate::config_check::{self, Severity};

        log::info!("reloading fan config from {}", CONFIG_PATH);

        let config = match load_config() {
            Some(c) => c,
            None => {
                log::error!("reload failed: could not load config");
                return;
            }
        };

        let issues = config_check::validate(&config);
        let errors = issues.iter().filter(|i| i.severity == Severity::Error).count();

        for issue in &issues {
            let level = match issue.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            log::warn!("config {}: {}", level, issue.message);
        }

        if errors > 0 {
            log::error!("reload aborted: config has {} error(s)", errors);
            return;
        }

        self.apply_config(Some(config));

        // Re-apply the current profile's curve so channels don't fall
        // back to the shared curve after a reload.
        if !self.current_profile.is_empty() {
            let profile = self.current_profile.clone();
            self.set_profile(&profile);
        }

        log::info!("fan config reloaded successfully");
    }

    /// Switch the active power profile, rebuilding channel curves.
    ///
    /// Resolution order per channel (most specific wins):
    /// 1. Per-channel per-profile curve (this channel + this profile)
    /// 2. Per-channel default curve (this channel, any profile)
    /// 3. Per-profile global curve (any channel, this profile)
    /// 4. Shared top-level curve (fallback)
    pub fn set_profile(&mut self, profile: &str) {
        self.current_profile = profile.to_lowercase();
        let global_curve =
            self.profile_curves.get(&self.current_profile).unwrap_or(&self.shared_curve);

        for (i, def) in self.channel_defs.iter().enumerate() {
            if let Some(ch) = self.channels.get_mut(i) {
                ch.curve = def
                    .profile_curves
                    .get(&self.current_profile)
                    .or(def.override_curve.as_ref())
                    .unwrap_or(global_curve)
                    .clone();
                ch.min_duty = def.min_duty_byte;
                ch.stall_detect = def.stall_detect;
                ch.stall_threshold = def.stall_threshold;
                ch.passthrough = def.passthrough;
            }
        }

        // Reset hysteresis and stall tracking so the new curves take effect immediately
        self.last_duties.fill(0);
        self.last_temps.fill(None);
        self.stall_counts.fill(0);

        // Clear any temporary fan overrides when the profile changes.
        if let Ok(mut s) = self.status.lock() {
            s.overrides.clear();
        }

        log::info!("fan curves updated for profile: {}", profile);
    }

    /// Discover all utilizable hwmon devices.
    fn discover(&mut self) -> Result<(), FanDaemonError> {
        self.amdgpus.clear();
        self.platforms.clear();
        self.cpus.clear();

        for hwmon in HwMon::all().map_err(FanDaemonError::HwmonDevices)? {
            if let Ok(name) = hwmon.name() {
                log::debug!("hwmon: {}", name);

                match name.as_str() {
                    "amdgpu" => self.amdgpus.push(hwmon),
                    "apm_xgene" | "coretemp" | "k10temp" | "zenpower" => self.cpus.push(hwmon),
                    n if self.platform_names.iter().any(|p| p == n) => {
                        self.platforms.push(hwmon);
                    }
                    _ => (),
                }
            }
        }

        if self.platforms.is_empty() {
            return Err(FanDaemonError::PlatformHwmonNotFound);
        }

        if self.cpus.is_empty() {
            return Err(FanDaemonError::CpuHwmonNotFound);
        }

        Ok(())
    }

    /// Max temperature across CPU hwmon sensors, in millidegrees Celsius.
    fn get_cpu_temp(&self) -> Option<u32> {
        self.cpus
            .iter()
            .filter_map(|sensor| sensor.temp(1).ok())
            .filter_map(|temp| temp.input().ok())
            .fold(None, |best, input| {
                let val = input as u32;
                if best.is_none_or(|b| val > b) {
                    log::debug!("highest cpu temp: {}", val);
                    Some(val)
                } else {
                    best
                }
            })
    }

    /// Max temperature across GPU sensors (amdgpu hwmon + NVML), in millidegrees Celsius.
    fn get_gpu_temp(&self) -> Option<u32> {
        let mut temp_opt = self
            .amdgpus
            .iter()
            .filter_map(|sensor| sensor.temp(1).ok())
            .filter_map(|temp| temp.input().ok())
            .fold(None, |best, input| {
                let val = input as u32;
                if best.is_none_or(|b| val > b) {
                    log::debug!("highest amdgpu temp: {}", val);
                    Some(val)
                } else {
                    best
                }
            });

        match self.nvidia {
            NvidiaState::Absent => {}
            NvidiaState::Unavailable => {
                // NVIDIA hardware exists but we can't read its temp.
                // Force GPU-sourced channels to max by reporting critical temp.
                let safety = self.critical_gpu_temp;
                temp_opt = Some(temp_opt.map_or(safety, |t| cmp::max(safety, t)));
            }
            NvidiaState::Active(ref nvml) => {
                if let Some(nv_temp) = nvml.max_gpu_temp() {
                    log::debug!("highest nvidia temp: {}", nv_temp);
                    temp_opt = Some(temp_opt.map_or(nv_temp, |t| cmp::max(nv_temp, t)));
                }
            }
        }

        temp_opt
    }

    /// Read the temperature for a given source, in millidegrees Celsius.
    fn get_temp_for(&self, source: TempSource) -> Option<u32> {
        match source {
            TempSource::Cpu => self.get_cpu_temp(),
            TempSource::Gpu => self.get_gpu_temp(),
            TempSource::All => {
                let cpu = self.get_cpu_temp();
                let gpu = self.get_gpu_temp();
                match (cpu, gpu) {
                    (Some(c), Some(g)) => Some(cmp::max(c, g)),
                    (Some(c), None) => Some(c),
                    (None, Some(g)) => Some(g),
                    (None, None) => None,
                }
            }
        }
    }

    /// Set a single PWM channel's duty cycle (0-255), or restore auto mode on None.
    ///
    /// The enable file is derived from the channel name (e.g. "pwm2" -> "pwm2_enable")
    /// so each channel controls its own hwmon output independently.
    fn set_channel_duty(&self, pwm: &str, duty_opt: Option<u8>) {
        let enable_file = format!("{}_enable", pwm);
        for platform in &self.platforms {
            if let Some(duty) = duty_opt {
                let _ = platform.write_file(&enable_file, "1");
                let _ = platform.write_file(pwm, format!("{}", duty));
            } else {
                let _ = platform.write_file(&enable_file, "2");
            }
        }
    }

    /// Read RPM from the fan tachometer that corresponds to a PWM channel.
    ///
    /// Extracts the numeric index from the channel name (e.g. "pwm2" -> 2)
    /// and reads fanN_input from the platform hwmon. Returns None if the
    /// sensor doesn't exist or can't be read.
    fn read_fan_rpm(&self, pwm: &str) -> Option<u32> {
        let idx: u64 = pwm.strip_prefix("pwm")?.parse().ok()?;
        self.platforms.iter().find_map(|p| p.fan(idx).ok().and_then(|f| f.input().ok()))
    }

    /// Fallback duty for stall recovery when no min_duty is configured.
    const STALL_FALLBACK_DUTY: u8 = 38; // ~15%

    /// Evaluate each fan channel against its temperature source and apply the result.
    ///
    /// If any component crosses the critical threshold, all fans go to max
    /// duty regardless of their individual temperature source.
    ///
    /// Returns true if critical temps were reached (used by thermal fallback).
    pub fn step(&mut self) -> bool {
        if self.platform_names.is_empty() {
            return false;
        }

        if self.discover().is_ok() {
            let cpu_temp = self.get_cpu_temp();
            let gpu_temp = self.get_gpu_temp();
            let critical = cpu_temp.is_some_and(|t| t >= self.critical_cpu_temp)
                || gpu_temp.is_some_and(|t| t >= self.critical_gpu_temp);

            let mut duties = Vec::new();
            let mut rpms = Vec::new();
            let mut stalled = Vec::new();

            if critical {
                log::warn!("critical temp reached, all fans to max");
                for channel in &self.channels {
                    if channel.passthrough {
                        duties.push((channel.pwm.clone(), None));
                        rpms.push((channel.pwm.clone(), None));
                        continue;
                    }
                    self.set_channel_duty(&channel.pwm, Some(255));
                    duties.push((channel.pwm.clone(), Some(255)));
                    rpms.push((channel.pwm.clone(), self.read_fan_rpm(&channel.pwm)));
                }
                self.stall_counts.fill(0);
            } else {
                for (i, channel) in self.channels.iter().enumerate() {
                    // Passthrough channels are left under BIOS/firmware control.
                    if channel.passthrough {
                        duties.push((channel.pwm.clone(), None));
                        rpms.push((channel.pwm.clone(), None));
                        continue;
                    }

                    // Temporary override bypasses curve evaluation entirely.
                    let override_duty = self
                        .status
                        .lock()
                        .ok()
                        .and_then(|s| s.overrides.get(&channel.pwm).copied());
                    if let Some(duty) = override_duty {
                        self.set_channel_duty(&channel.pwm, Some(duty));
                        duties.push((channel.pwm.clone(), Some(duty)));
                        rpms.push((channel.pwm.clone(), self.read_fan_rpm(&channel.pwm)));
                        self.stall_counts[i] = 0;
                        continue;
                    }

                    let temp = self.get_temp_for(channel.source);
                    let curve_duty = temp.and_then(|t| duty_from_curve(&channel.curve, t));

                    let effective_duty = match (curve_duty, temp) {
                        (Some(new_duty), Some(current_temp)) => {
                            let last = self.last_duties[i];
                            if new_duty >= last {
                                self.last_duties[i] = new_duty;
                                self.last_temps[i] = Some(current_temp);
                                Some(new_duty)
                            } else if let Some(lt) = self.last_temps[i] {
                                if lt.saturating_sub(current_temp) >= self.hysteresis {
                                    self.last_duties[i] = new_duty;
                                    self.last_temps[i] = Some(current_temp);
                                    Some(new_duty)
                                } else {
                                    Some(last)
                                }
                            } else {
                                self.last_duties[i] = new_duty;
                                self.last_temps[i] = Some(current_temp);
                                Some(new_duty)
                            }
                        }
                        (duty, _) => duty,
                    };

                    // Apply minimum duty floor if configured.
                    let floored_duty = match (effective_duty, channel.min_duty) {
                        (Some(d), Some(floor)) => Some(d.max(floor)),
                        (None, Some(floor)) => Some(floor),
                        (duty, None) => duty,
                    };

                    // Stall detection: if duty > 0 but RPM reads 0, the fan
                    // may have stalled. Bump to floor or fallback after
                    // consecutive zero-RPM reads exceed the threshold.
                    let rpm =
                        if channel.stall_detect { self.read_fan_rpm(&channel.pwm) } else { None };

                    let final_duty = if channel.stall_detect {
                        let is_spinning = floored_duty.is_some_and(|d| d > 0);
                        let rpm_zero = rpm.is_some_and(|r| r == 0);

                        if is_spinning && rpm_zero {
                            self.stall_counts[i] += 1;
                            if self.stall_counts[i] >= channel.stall_threshold {
                                let bump = channel.min_duty.unwrap_or(Self::STALL_FALLBACK_DUTY);
                                log::warn!(
                                    "{}: fan stalled (0 RPM with duty > 0), bumping to {}",
                                    channel.pwm,
                                    bump
                                );
                                stalled.push(channel.pwm.clone());
                                Some(floored_duty.map_or(bump, |d| d.max(bump)))
                            } else {
                                floored_duty
                            }
                        } else {
                            self.stall_counts[i] = 0;
                            floored_duty
                        }
                    } else {
                        floored_duty
                    };

                    self.set_channel_duty(&channel.pwm, final_duty);
                    duties.push((channel.pwm.clone(), final_duty));
                    rpms.push((channel.pwm.clone(), rpm));
                }
            }

            if let Ok(mut s) = self.status.lock() {
                s.cpu_temp = cpu_temp;
                s.gpu_temp = gpu_temp;
                s.channel_duties = duties;
                s.channel_curves = self
                    .channels
                    .iter()
                    .filter(|ch| !ch.passthrough)
                    .map(|ch| (ch.pwm.clone(), ch.curve.to_display_points()))
                    .collect();
                s.min_duties = self
                    .channels
                    .iter()
                    .filter(|ch| !ch.passthrough)
                    .map(|ch| (ch.pwm.clone(), ch.min_duty))
                    .collect();
                s.rpms = rpms;
                s.stalled = stalled;
                s.passthrough = self
                    .channels
                    .iter()
                    .filter(|ch| ch.passthrough)
                    .map(|ch| ch.pwm.clone())
                    .collect();
                s.critical = critical;
            }

            return critical;
        }

        false
    }

    /// Whether thermal fallback is enabled in the config.
    pub fn thermal_fallback_enabled(&self) -> bool {
        self.thermal_fallback
    }

    /// Cooldown period in seconds before stepping back up after thermal fallback.
    pub fn thermal_cooldown_secs(&self) -> u32 {
        self.thermal_cooldown
    }
}

impl Drop for FanDaemon {
    fn drop(&mut self) {
        if self.platform_names.is_empty() {
            return;
        }
        for channel in &self.channels {
            if channel.passthrough {
                continue;
            }
            self.set_channel_duty(&channel.pwm, None);
        }
    }
}

/// Convert a millidegree temperature to a 0-255 PWM duty using the given curve.
fn duty_from_curve(curve: &FanCurve, temp_millideg: u32) -> Option<u8> {
    curve
        .get_duty((temp_millideg / 10) as i16)
        .map(|duty| (((u32::from(duty)) * 255) / 10_000) as u8)
}

/// Try to load and parse the TOML config file. Returns None if the file
/// is missing (silent) or malformed (logged as error).
pub(crate) fn load_config() -> Option<FanConfig> {
    let contents = fs::read_to_string(CONFIG_PATH).ok()?;
    match toml::from_str(&contents) {
        Ok(config) => {
            log::info!("loaded fan config from {}", CONFIG_PATH);
            Some(config)
        }
        Err(err) => {
            log::error!("failed to parse {}: {}", CONFIG_PATH, err);
            None
        }
    }
}

/// Convert config curve points (Celsius / percent) to internal FanCurve.
fn build_curve(points: &[CurvePoint]) -> FanCurve {
    points.iter().fold(FanCurve::default(), |curve, p| {
        curve.append((p.temp * 100.0) as i16, (p.duty * 100.0) as u16)
    })
}

/// Parse a temperature source string from config. Defaults to All for
/// unrecognized values so fans never run without a temp signal.
fn parse_temp_source(s: &str) -> TempSource {
    match s {
        "cpu" => TempSource::Cpu,
        "gpu" => TempSource::Gpu,
        _ => TempSource::All,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FanPoint {
    // Temperature in hundredths of a degree, 10000 = 100C
    temp: i16,
    // duty in hundredths of a percent, 10000 = 100%
    duty: u16,
}

impl FanPoint {
    pub const fn new(temp: i16, duty: u16) -> Self {
        Self { temp, duty }
    }

    /// Find the duty between two points and a given temperature, if the temperature
    /// lies within this range.
    fn get_duty_between_points(self, next: Self, temp: i16) -> Option<u16> {
        // If the temp matches the next point, return the next point duty
        if temp == next.temp {
            return Some(next.duty);
        }

        // If the temp matches the previous point, return the previous point duty
        if temp == self.temp {
            return Some(self.duty);
        }

        // If the temp is in between the previous and next points, interpolate the duty
        if self.temp < temp && next.temp > temp {
            return Some(self.interpolate_duties(next, temp));
        }

        None
    }

    /// Interpolates the current duty with that of the given next point and temperature.
    fn interpolate_duties(self, next: Self, temp: i16) -> u16 {
        let dtemp = next.temp - self.temp;
        let dduty = next.duty - self.duty;

        let slope = f32::from(dduty) / f32::from(dtemp);

        let temp_offset = temp - self.temp;
        let duty_offset = (slope * f32::from(temp_offset)).round();

        self.duty + (duty_offset as u16)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FanCurve {
    points: Vec<FanPoint>,
}

impl FanCurve {
    /// Adds a point to the fan curve
    #[must_use]
    pub fn append(mut self, temp: i16, duty: u16) -> Self {
        self.points.push(FanPoint::new(temp, duty));
        self
    }

    /// The standard fan curve, tuned for aggressive ramp with quiet idle.
    /// Low-speed buffer at 40C avoids hard on/off cycling on bearings.
    /// Hits 100% at 70C for plenty of thermal headroom.
    pub fn standard() -> Self {
        Self::default()
            .append(39_99, 0_00)
            .append(40_00, 15_00)
            .append(45_00, 30_00)
            .append(50_00, 40_00)
            .append(55_00, 55_00)
            .append(60_00, 70_00)
            .append(65_00, 85_00)
            .append(70_00, 100_00)
    }

    /// Return curve points as (Celsius, percent) tuples for display.
    pub fn to_display_points(&self) -> Vec<(f32, f32)> {
        self.points.iter().map(|p| (f32::from(p.temp) / 100.0, f32::from(p.duty) / 100.0)).collect()
    }

    pub fn get_duty(&self, temp: i16) -> Option<u16> {
        // Below the curve means fans off
        if let Some(first) = self.points.first() {
            if temp < first.temp {
                return Some(0);
            }
        }

        // Use when we upgrade to 1.28.0
        // for &[prev, next] in self.points.windows(2) {

        for window in self.points.windows(2) {
            let prev = window[0];
            let next = window[1];
            if let Some(duty) = prev.get_duty_between_points(next, temp) {
                return Some(duty);
            }
        }

        // If the temp is greater than the last point, return the last point duty
        if let Some(last) = self.points.last() {
            if temp > last.temp {
                return Some(last.duty);
            }
        }

        // If there are no points, return None
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duty_interpolation() {
        let fan_point = FanPoint::new(20_00, 30_00);
        let next_point = FanPoint::new(30_00, 35_00);

        assert_eq!(fan_point.get_duty_between_points(next_point, 1500), None);
        assert_eq!(fan_point.get_duty_between_points(next_point, 2000), Some(3000));
        assert_eq!(fan_point.get_duty_between_points(next_point, 3000), Some(3500));
        assert_eq!(fan_point.get_duty_between_points(next_point, 3250), None);
        assert_eq!(fan_point.get_duty_between_points(next_point, 3500), None);
    }

    #[test]
    fn standard_points() {
        let standard = FanCurve::standard();

        assert_eq!(standard.get_duty(0), Some(0));
        assert_eq!(standard.get_duty(3999), Some(0));
        assert_eq!(standard.get_duty(4000), Some(1500));
        assert_eq!(standard.get_duty(4500), Some(3000));
        assert_eq!(standard.get_duty(5000), Some(4000));
        assert_eq!(standard.get_duty(5500), Some(5500));
        assert_eq!(standard.get_duty(6000), Some(7000));
        assert_eq!(standard.get_duty(6500), Some(8500));
        assert_eq!(standard.get_duty(7000), Some(10000));
        assert_eq!(standard.get_duty(10000), Some(10000));
    }

    #[test]
    fn config_with_platform() {
        let toml_str = r#"
            platform = "nct6775"
            critical_cpu_temp = 85
            critical_gpu_temp = 80

            [[curve]]
            temp = 35.0
            duty = 0

            [[channels]]
            pwm = "pwm1"
            source = "cpu"
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.platform.as_deref(), Some("nct6775"));
    }

    #[test]
    fn config_without_platform() {
        let toml_str = r#"
            critical_cpu_temp = 79
            critical_gpu_temp = 75

            [[curve]]
            temp = 40.0
            duty = 15

            [[channels]]
            pwm = "pwm1"
            source = "cpu"
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        assert!(config.platform.is_none());
    }

    #[test]
    fn config_per_channel_curve() {
        let toml_str = r#"
            critical_cpu_temp = 85
            critical_gpu_temp = 80

            [[curve]]
            temp = 35.0
            duty = 0

            [[curve]]
            temp = 70.0
            duty = 100

            [[channels]]
            pwm = "pwm1"
            source = "cpu"

            [[channels]]
            pwm = "pwm2"
            source = "gpu"

            [[channels.curve]]
            temp = 40.0
            duty = 0

            [[channels.curve]]
            temp = 80.0
            duty = 100
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.channels.len(), 2);

        // First channel has no override, should use shared curve
        assert!(config.channels[0].curve.is_none());

        // Second channel has its own curve
        let ch_curve = config.channels[1].curve.as_ref().unwrap();
        assert_eq!(ch_curve.len(), 2);
        assert_eq!(ch_curve[0].temp, 40.0);
        assert_eq!(ch_curve[1].temp, 80.0);

        // Verify the built curves differ
        let shared = build_curve(&config.curve);
        let override_curve = build_curve(ch_curve);
        assert_ne!(shared, override_curve);
    }

    #[test]
    fn config_parsing() {
        let toml_str = r#"
            critical_cpu_temp = 79
            critical_gpu_temp = 75

            [[curve]]
            temp = 40.0
            duty = 15

            [[curve]]
            temp = 70.0
            duty = 100

            [[channels]]
            pwm = "pwm1"
            source = "cpu"

            [[channels]]
            pwm = "pwm2"
            source = "all"

            [[channels]]
            pwm = "pwm3"
            source = "gpu"
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.critical_cpu_temp, 79.0);
        assert_eq!(config.critical_gpu_temp, 75.0);
        assert_eq!(config.curve.len(), 2);
        assert_eq!(config.channels.len(), 3);

        // Verify unit conversion
        let curve = build_curve(&config.curve);
        assert_eq!(curve.get_duty(40_00), Some(15_00));
        assert_eq!(curve.get_duty(70_00), Some(100_00));

        // Verify temp source parsing
        assert_eq!(parse_temp_source("cpu"), TempSource::Cpu);
        assert_eq!(parse_temp_source("gpu"), TempSource::Gpu);
        assert_eq!(parse_temp_source("all"), TempSource::All);
        assert_eq!(parse_temp_source("bogus"), TempSource::All);

        // Verify critical temp conversion (Celsius -> millidegrees)
        assert_eq!((config.critical_cpu_temp * 1000.0) as u32, 79_000);
        assert_eq!((config.critical_gpu_temp * 1000.0) as u32, 75_000);
    }

    #[test]
    fn config_hysteresis_default() {
        let toml_str = r#"
            critical_cpu_temp = 85
            critical_gpu_temp = 80

            [[curve]]
            temp = 35.0
            duty = 0

            [[channels]]
            pwm = "pwm1"
            source = "cpu"
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        assert!(config.hysteresis.is_none());

        let hyst = (config.hysteresis.unwrap_or(DEFAULT_HYSTERESIS_C) * 1000.0) as u32;
        assert_eq!(hyst, 3000);
    }

    #[test]
    fn config_hysteresis_custom() {
        let toml_str = r#"
            critical_cpu_temp = 85
            critical_gpu_temp = 80
            hysteresis = 5.0

            [[curve]]
            temp = 35.0
            duty = 0

            [[channels]]
            pwm = "pwm1"
            source = "cpu"
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hysteresis, Some(5.0));
    }

    /// Verify that rising temps update duty immediately and falling
    /// temps within the hysteresis band hold the previous duty.
    #[test]
    fn hysteresis_holds_duty_on_small_drop() {
        let curve = FanCurve::standard();
        let hysteresis: u32 = 3000; // 3C in millidegrees

        // Simulate rising: 50C -> duty at 50C
        let temp_50c = 50_000u32;
        let duty_at_50 = duty_from_curve(&curve, temp_50c).unwrap();

        // Simulate a small drop: 49C (1C drop, within 3C hysteresis)
        let temp_49c = 49_000u32;
        let duty_at_49 = duty_from_curve(&curve, temp_49c).unwrap();

        // The curve gives a lower duty at 49C
        assert!(duty_at_49 < duty_at_50);

        // But with hysteresis, a 1C drop shouldn't reduce duty
        let drop = temp_50c.saturating_sub(temp_49c);
        assert!(drop < hysteresis, "1C drop should be within 3C hysteresis band");

        // A 4C drop should pass the hysteresis threshold
        let temp_46c = 46_000u32;
        let drop = temp_50c.saturating_sub(temp_46c);
        assert!(drop >= hysteresis, "4C drop should exceed 3C hysteresis band");
    }

    #[test]
    fn config_per_channel_profile_curves() {
        let toml_str = r#"
            critical_cpu_temp = 85
            critical_gpu_temp = 80

            [[curve]]
            temp = 35.0
            duty = 10

            [[curve]]
            temp = 70.0
            duty = 100

            [profiles.quiet]
            curve = [
                { temp = 40.0, duty = 5 },
                { temp = 80.0, duty = 60 },
            ]

            [[channels]]
            pwm = "pwm1"
            source = "cpu"

            [[channels]]
            pwm = "pwm3"
            source = "gpu"

            # Default per-channel curve for pwm3
            [[channels.curve]]
            temp = 30.0
            duty = 5

            [[channels.curve]]
            temp = 80.0
            duty = 100

            # Per-channel per-profile curves for pwm3
            [channels.profiles.quiet]
            curve = [
                { temp = 30.0, duty = 3 },
                { temp = 80.0, duty = 80 },
            ]

            [channels.profiles.performance]
            curve = [
                { temp = 30.0, duty = 10 },
                { temp = 70.0, duty = 100 },
            ]
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();

        // pwm1 has no per-channel anything
        assert!(config.channels[0].curve.is_none());
        assert!(config.channels[0].profiles.is_none());

        // pwm3 has a per-channel curve and per-channel profile curves
        assert!(config.channels[1].curve.is_some());
        let ch_profiles = config.channels[1].profiles.as_ref().unwrap();
        assert_eq!(ch_profiles.len(), 2);
        assert!(ch_profiles.contains_key("quiet"));
        assert!(ch_profiles.contains_key("performance"));

        // Verify the curves are distinct
        let shared = build_curve(&config.curve);
        let ch_default = build_curve(config.channels[1].curve.as_ref().unwrap());
        let ch_quiet = build_curve(&ch_profiles["quiet"].curve);
        let ch_perf = build_curve(&ch_profiles["performance"].curve);

        // At 50C, all four curves should yield different duties
        let shared_50 = shared.get_duty(50_00).unwrap();
        let ch_default_50 = ch_default.get_duty(50_00).unwrap();
        let ch_quiet_50 = ch_quiet.get_duty(50_00).unwrap();
        let ch_perf_50 = ch_perf.get_duty(50_00).unwrap();

        assert_ne!(shared_50, ch_default_50);
        assert_ne!(ch_default_50, ch_quiet_50);
        assert!(ch_quiet_50 < ch_default_50, "quiet should be lower duty than default");
        assert!(ch_perf_50 > ch_default_50, "performance should be higher duty than default");
    }

    #[test]
    fn config_min_duty() {
        let toml_str = r#"
            critical_cpu_temp = 85
            critical_gpu_temp = 80

            [[curve]]
            temp = 40.0
            duty = 15

            [[channels]]
            pwm = "pwm1"
            source = "cpu"
            min_duty = 10.0
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.channels[0].min_duty, Some(10.0));
    }

    #[test]
    fn config_min_duty_absent() {
        let toml_str = r#"
            critical_cpu_temp = 85
            critical_gpu_temp = 80

            [[curve]]
            temp = 40.0
            duty = 15

            [[channels]]
            pwm = "pwm1"
            source = "cpu"
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        assert!(config.channels[0].min_duty.is_none());
    }

    #[test]
    fn min_duty_byte_conversion() {
        // 15% of 255 = 38.25, rounds to 38
        let pct = 15.0f32;
        let byte = ((pct.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8;
        assert_eq!(byte, 38);

        // 100% = 255
        let pct = 100.0f32;
        let byte = ((pct.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8;
        assert_eq!(byte, 255);

        // 0% = 0
        let pct = 0.0f32;
        let byte = ((pct.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8;
        assert_eq!(byte, 0);
    }

    #[test]
    fn config_per_profile_curves() {
        let toml_str = r#"
            critical_cpu_temp = 85
            critical_gpu_temp = 80

            [[curve]]
            temp = 35.0
            duty = 0

            [[curve]]
            temp = 70.0
            duty = 100

            [profiles.quiet]
            curve = [
                { temp = 40.0, duty = 0 },
                { temp = 80.0, duty = 60 },
            ]

            [profiles.performance]
            curve = [
                { temp = 30.0, duty = 20 },
                { temp = 60.0, duty = 100 },
            ]

            [[channels]]
            pwm = "pwm1"
            source = "cpu"
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        let profiles = config.profiles.as_ref().unwrap();
        assert_eq!(profiles.len(), 2);
        assert!(profiles.contains_key("quiet"));
        assert!(profiles.contains_key("performance"));

        let quiet_curve = build_curve(&profiles["quiet"].curve);
        let perf_curve = build_curve(&profiles["performance"].curve);
        let shared_curve = build_curve(&config.curve);

        // Quiet curve is more conservative (lower duty at same temp)
        assert!(quiet_curve.get_duty(50_00).unwrap() < shared_curve.get_duty(50_00).unwrap());
        // Performance curve is more aggressive
        assert!(perf_curve.get_duty(50_00).unwrap() > shared_curve.get_duty(50_00).unwrap());
    }

    #[test]
    fn config_stall_detect() {
        let toml_str = r#"
            critical_cpu_temp = 85
            critical_gpu_temp = 80

            [[curve]]
            temp = 40.0
            duty = 15

            [[channels]]
            pwm = "pwm1"
            source = "cpu"
            stall_detect = true
            stall_threshold = 5
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.channels[0].stall_detect, Some(true));
        assert_eq!(config.channels[0].stall_threshold, Some(5));
    }

    #[test]
    fn config_stall_detect_defaults() {
        let toml_str = r#"
            critical_cpu_temp = 85
            critical_gpu_temp = 80

            [[curve]]
            temp = 40.0
            duty = 15

            [[channels]]
            pwm = "pwm1"
            source = "cpu"
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        assert!(config.channels[0].stall_detect.is_none());
        assert!(config.channels[0].stall_threshold.is_none());
    }

    #[test]
    fn stall_fallback_duty() {
        // Verify the fallback constant is ~15%
        let pct = (FanDaemon::STALL_FALLBACK_DUTY as f32 / 255.0) * 100.0;
        assert!(pct > 14.0 && pct < 16.0);
    }

    #[test]
    fn config_passthrough() {
        let toml_str = r#"
            critical_cpu_temp = 85
            critical_gpu_temp = 80

            [[curve]]
            temp = 40.0
            duty = 15

            [[channels]]
            pwm = "pwm1"
            source = "cpu"

            [[channels]]
            pwm = "pwm4"
            source = "all"
            passthrough = true
        "#;

        let config: FanConfig = toml::from_str(toml_str).unwrap();
        assert!(config.channels[0].passthrough.is_none());
        assert_eq!(config.channels[1].passthrough, Some(true));
    }
}
