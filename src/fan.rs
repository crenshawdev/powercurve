// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

#![allow(clippy::inconsistent_digit_grouping)]

use crate::nvml::NvidiaState;
use serde::Deserialize;
use std::{cmp, fs, io};
use sysfs_class::{HwMon, SysClass};

const CONFIG_PATH: &str = "/etc/vintagetechie-power/fan.toml";

// -- TOML config deserialization types --

/// Top-level config file structure.
#[derive(Deserialize)]
struct FanConfig {
    platform:          Option<String>,
    critical_cpu_temp: f32,
    critical_gpu_temp: f32,
    curve:             Vec<CurvePoint>,
    channels:          Vec<ChannelConfig>,
}

/// A single point on the fan curve. Human-friendly units.
#[derive(Deserialize)]
struct CurvePoint {
    /// Temperature in Celsius.
    temp: f32,
    /// Duty cycle as a percentage (0-100).
    duty: f32,
}

/// Maps a PWM output to a temperature source, with an optional per-channel curve.
#[derive(Deserialize)]
struct ChannelConfig {
    pwm:    String,
    source: String,
    curve:  Option<Vec<CurvePoint>>,
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
    pub pwm:    String,
    pub source: TempSource,
    pub curve:  FanCurve,
}

pub struct FanDaemon {
    channels:          Vec<FanChannel>,
    critical_cpu_temp: u32,
    critical_gpu_temp: u32,
    platform_names:    Vec<String>,
    amdgpus:           Vec<HwMon>,
    platforms:         Vec<HwMon>,
    cpus:              Vec<HwMon>,
    nvidia:            NvidiaState,
}

impl FanDaemon {
    /// Build a new fan daemon with per-channel temperature routing.
    ///
    /// Requires a config file at `/etc/vintagetechie-power/fan.toml`.
    /// Without one, fan control is disabled and the daemon only handles
    /// power profiles.
    pub fn new(nvidia: NvidiaState) -> Self {
        let config = load_config();

        let (channels, critical_cpu_temp, critical_gpu_temp, platform_names) =
            if let Some(config) = config {
                let shared_curve = build_curve(&config.curve);
                let channels = config
                    .channels
                    .into_iter()
                    .map(|ch| {
                        let curve = ch.curve.as_deref()
                            .map(build_curve)
                            .unwrap_or_else(|| shared_curve.clone());
                        FanChannel {
                            pwm:    ch.pwm,
                            source: parse_temp_source(&ch.source),
                            curve,
                        }
                    })
                    .collect();
                let cpu_crit = (config.critical_cpu_temp * 1000.0) as u32;
                let gpu_crit = (config.critical_gpu_temp * 1000.0) as u32;
                let platform = config.platform
                    .map(|name| vec![name])
                    .unwrap_or_default();
                (channels, cpu_crit, gpu_crit, platform)
            } else {
                log::warn!(
                    "no fan config found at {}, fan control disabled. \
                     run `vintagetechie-power fan-detect --generate` to create one",
                    CONFIG_PATH
                );
                (Vec::new(), 0, 0, Vec::new())
            };

        let mut daemon = Self {
            channels,
            critical_cpu_temp,
            critical_gpu_temp,
            platform_names,
            amdgpus: Vec::new(),
            platforms: Vec::new(),
            cpus: Vec::new(),
            nvidia,
        };

        if !daemon.platform_names.is_empty() {
            if let Err(err) = daemon.discover() {
                log::error!("fan daemon: {}", err);
            }
        }

        daemon
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

    /// Evaluate each fan channel against its temperature source and apply the result.
    ///
    /// If any component crosses the critical threshold, all fans go to max
    /// duty regardless of their individual temperature source.
    pub fn step(&mut self) {
        if self.platform_names.is_empty() {
            return;
        }

        if self.discover().is_ok() {
            let cpu_temp = self.get_cpu_temp();
            let gpu_temp = self.get_gpu_temp();
            let critical = cpu_temp.is_some_and(|t| t >= self.critical_cpu_temp)
                || gpu_temp.is_some_and(|t| t >= self.critical_gpu_temp);

            if critical {
                log::warn!("critical temp reached, all fans to max");
                for channel in &self.channels {
                    self.set_channel_duty(&channel.pwm, Some(255));
                }
            } else {
                for channel in &self.channels {
                    let temp = self.get_temp_for(channel.source);
                    let duty = temp.and_then(|t| duty_from_curve(&channel.curve, t));
                    self.set_channel_duty(&channel.pwm, duty);
                }
            }
        }
    }
}

impl Drop for FanDaemon {
    fn drop(&mut self) {
        if self.platform_names.is_empty() {
            return;
        }
        for channel in &self.channels {
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
fn load_config() -> Option<FanConfig> {
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
    pub const fn new(temp: i16, duty: u16) -> Self { Self { temp, duty } }

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
}
