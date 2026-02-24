// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

#![allow(clippy::inconsistent_digit_grouping)]

use serde::Deserialize;
use std::{
    cell::Cell,
    cmp, fs, io,
    process::{Command, Stdio},
};
use sysfs_class::{HwMon, SysClass};

const CONFIG_PATH: &str = "/etc/vintagetechie-power/fan.toml";

/// Default CPU critical threshold when no config file exists. 79C in millidegrees.
const DEFAULT_CRITICAL_CPU_TEMP: u32 = 79_000;

/// Default GPU critical threshold when no config file exists. 75C in millidegrees.
const DEFAULT_CRITICAL_GPU_TEMP: u32 = 75_000;

// -- TOML config deserialization types --

/// Top-level config file structure.
#[derive(Deserialize)]
struct FanConfig {
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

/// Maps a PWM output to a temperature source.
#[derive(Deserialize)]
struct ChannelConfig {
    pwm:    String,
    source: String,
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
    /// Max of GPU sensors (amdgpu hwmon + nvidia-smi).
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
    amdgpus:           Vec<HwMon>,
    platforms:         Vec<HwMon>,
    cpus:              Vec<HwMon>,
    nvidia_exists:     bool,
    displayed_warning: Cell<bool>,
}

impl FanDaemon {
    /// Build a new fan daemon with per-channel temperature routing.
    ///
    /// Tries to load config from `/etc/vintagetechie-power/fan.toml` first.
    /// Falls back to hardcoded defaults based on DMI product version
    /// if the config file is missing or malformed.
    pub fn new(nvidia_exists: bool) -> Self {
        let (channels, critical_cpu_temp, critical_gpu_temp) =
            if let Some(config) = load_config() {
                let curve = build_curve(&config.curve);
                let channels = config
                    .channels
                    .into_iter()
                    .map(|ch| FanChannel {
                        pwm:    ch.pwm,
                        source: parse_temp_source(&ch.source),
                        curve:  curve.clone(),
                    })
                    .collect();
                let cpu_crit = (config.critical_cpu_temp * 1000.0) as u32;
                let gpu_crit = (config.critical_gpu_temp * 1000.0) as u32;
                (channels, cpu_crit, gpu_crit)
            } else {
                (default_channels(), DEFAULT_CRITICAL_CPU_TEMP, DEFAULT_CRITICAL_GPU_TEMP)
            };

        let mut daemon = Self {
            channels,
            critical_cpu_temp,
            critical_gpu_temp,
            amdgpus: Vec::new(),
            platforms: Vec::new(),
            cpus: Vec::new(),
            nvidia_exists,
            displayed_warning: Cell::new(false),
        };

        if let Err(err) = daemon.discover() {
            log::error!("fan daemon: {}", err);
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
                    "system76" => (),
                    "system76_io" | "system76_thelio_io" => self.platforms.push(hwmon),
                    "apm_xgene" | "coretemp" | "k10temp" | "zenpower" => self.cpus.push(hwmon),
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
                if best.map_or(true, |b| val > b) {
                    log::debug!("highest cpu temp: {}", val);
                    Some(val)
                } else {
                    best
                }
            })
    }

    /// Max temperature across GPU sensors (amdgpu hwmon + nvidia-smi), in millidegrees Celsius.
    fn get_gpu_temp(&self) -> Option<u32> {
        let mut temp_opt = self
            .amdgpus
            .iter()
            .filter_map(|sensor| sensor.temp(1).ok())
            .filter_map(|temp| temp.input().ok())
            .fold(None, |best, input| {
                let val = input as u32;
                if best.map_or(true, |b| val > b) {
                    log::debug!("highest amdgpu temp: {}", val);
                    Some(val)
                } else {
                    best
                }
            });

        // nvidia-smi reports in whole Celsius, convert to millidegrees
        // to match the hwmon convention used everywhere else.
        if self.nvidia_exists && !self.displayed_warning.get() {
            let mut nv_temp = 0;
            match nvidia_temperatures(|temp| nv_temp = cmp::max(temp, nv_temp)) {
                Ok(()) => {
                    if nv_temp != 0 {
                        let nv_temp_m = nv_temp * 1000;
                        log::debug!("highest nvidia temp: {}", nv_temp_m);
                        temp_opt =
                            Some(temp_opt.map_or(nv_temp_m, |t| cmp::max(nv_temp_m, t)));
                    }
                }
                Err(why) => {
                    log::warn!("failed to get temperature of NVIDIA GPUs: {}", why);
                    self.displayed_warning.set(true);
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
    fn set_channel_duty(&self, pwm: &str, duty_opt: Option<u8>) {
        for platform in &self.platforms {
            if let Some(duty) = duty_opt {
                let _ = platform.write_file("pwm1_enable", "1");
                let _ = platform.write_file(pwm, &format!("{}", duty));
            } else {
                let _ = platform.write_file("pwm1_enable", "2");
            }
        }
    }

    /// Evaluate each fan channel against its temperature source and apply the result.
    ///
    /// If any component crosses the critical threshold, all fans go to max
    /// duty regardless of their individual temperature source.
    pub fn step(&mut self) {
        if self.discover().is_ok() {
            let cpu_temp = self.get_cpu_temp();
            let gpu_temp = self.get_gpu_temp();
            let critical = cpu_temp.map_or(false, |t| t >= self.critical_cpu_temp)
                || gpu_temp.map_or(false, |t| t >= self.critical_gpu_temp);

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

/// Hardcoded channel layout used when no config file exists.
/// Falls back to DMI model detection for curve selection.
fn default_channels() -> Vec<FanChannel> {
    let model = fs::read_to_string("/sys/class/dmi/id/product_version").unwrap_or_default();

    match model.trim() {
        "thelio-major-r1" => {
            all_channels_uniform(FanCurve::threadripper2())
        }
        "thelio-astra-a1" | "thelio-astra-a1.1" | "thelio-major-r2"
        | "thelio-major-r2.1" | "thelio-major-b1" | "thelio-major-b2"
        | "thelio-major-b3" | "thelio-mega-r1" | "thelio-mega-r1.1" => {
            all_channels_uniform(FanCurve::hedt())
        }
        "thelio-massive-b1" => {
            all_channels_uniform(FanCurve::xeon())
        }
        _ => {
            let curve = FanCurve::standard();
            vec![
                FanChannel { pwm: "pwm1".into(), source: TempSource::Cpu, curve: curve.clone() },
                FanChannel { pwm: "pwm2".into(), source: TempSource::All, curve: curve.clone() },
                FanChannel { pwm: "pwm3".into(), source: TempSource::Gpu, curve },
            ]
        }
    }
}

/// Build an all-channels-same layout for models without per-component mapping.
fn all_channels_uniform(curve: FanCurve) -> Vec<FanChannel> {
    vec![
        FanChannel { pwm: "pwm1".into(), source: TempSource::All, curve: curve.clone() },
        FanChannel { pwm: "pwm2".into(), source: TempSource::All, curve: curve.clone() },
        FanChannel { pwm: "pwm3".into(), source: TempSource::All, curve: curve.clone() },
        FanChannel { pwm: "pwm4".into(), source: TempSource::All, curve },
    ]
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

    /// Fan curve for threadripper 2
    pub fn threadripper2() -> Self {
        Self::default()
            .append(00_00, 30_00)
            .append(40_00, 40_00)
            .append(47_50, 50_00)
            .append(55_00, 65_00)
            .append(62_50, 85_00)
            .append(66_25, 100_00)
    }

    /// Fan curve for HEDT systems
    pub fn hedt() -> Self {
        Self::default()
            .append(00_00, 30_00)
            .append(50_00, 35_00)
            .append(60_00, 45_00)
            .append(70_00, 55_00)
            .append(74_00, 60_00)
            .append(76_00, 70_00)
            .append(78_00, 80_00)
            .append(81_00, 100_00)
    }

    /// Fan curve for xeon
    pub fn xeon() -> Self {
        Self::default()
            .append(00_00, 40_00)
            .append(50_00, 40_00)
            .append(55_00, 45_00)
            .append(60_00, 50_00)
            .append(65_00, 55_00)
            .append(70_00, 60_00)
            .append(72_00, 65_00)
            .append(74_00, 80_00)
            .append(76_00, 85_00)
            .append(77_00, 90_00)
            .append(78_00, 100_00)
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

pub fn nvidia_temperatures<F: FnMut(u32)>(func: F) -> io::Result<()> {
    let output = Command::new("nvidia-smi")
        .arg("--query-gpu=temperature.gpu")
        .arg("--format=csv,noheader")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()?;

    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "non-utf8 output"))?;

    stdout.lines().filter_map(|line| line.parse::<u32>().ok()).for_each(func);

    Ok(())
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
    fn hedt_points() {
        let hedt = FanCurve::hedt();

        assert_eq!(hedt.get_duty(0), Some(3000));
        assert_eq!(hedt.get_duty(5000), Some(3500));
        assert_eq!(hedt.get_duty(6000), Some(4500));
        assert_eq!(hedt.get_duty(7000), Some(5500));
        assert_eq!(hedt.get_duty(7400), Some(6000));
        assert_eq!(hedt.get_duty(7600), Some(7000));
        assert_eq!(hedt.get_duty(7800), Some(8000));
        assert_eq!(hedt.get_duty(8100), Some(10000));
        assert_eq!(hedt.get_duty(10000), Some(10000));
    }

    #[test]
    fn threadripper2_points() {
        let threadripper2 = FanCurve::threadripper2();

        assert_eq!(threadripper2.get_duty(0), Some(3000));
        assert_eq!(threadripper2.get_duty(4000), Some(4000));
        assert_eq!(threadripper2.get_duty(4750), Some(5000));
        assert_eq!(threadripper2.get_duty(5500), Some(6500));
        assert_eq!(threadripper2.get_duty(6250), Some(8500));
        assert_eq!(threadripper2.get_duty(6625), Some(10000));
        assert_eq!(threadripper2.get_duty(10000), Some(10000));
    }

    #[test]
    fn xeon_points() {
        let xeon = FanCurve::xeon();

        assert_eq!(xeon.get_duty(0), Some(4000));
        assert_eq!(xeon.get_duty(5000), Some(4000));
        assert_eq!(xeon.get_duty(5500), Some(4500));
        assert_eq!(xeon.get_duty(6000), Some(5000));
        assert_eq!(xeon.get_duty(6500), Some(5500));
        assert_eq!(xeon.get_duty(7000), Some(6000));
        assert_eq!(xeon.get_duty(7200), Some(6500));
        assert_eq!(xeon.get_duty(7400), Some(8000));
        assert_eq!(xeon.get_duty(7600), Some(8500));
        assert_eq!(xeon.get_duty(7700), Some(9000));
        assert_eq!(xeon.get_duty(7800), Some(10000));
        assert_eq!(xeon.get_duty(10000), Some(10000));
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
