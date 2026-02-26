// SPDX-License-Identifier: GPL-3.0-only

use crate::fan::{self, CONFIG_PATH, FanConfig};
use std::fs;
use sysfs_class::{HwMon, SysClass};

/// Severity level for config validation results.
#[derive(PartialEq)]
pub(crate) enum Severity {
    Error,
    Warning,
}

pub(crate) struct Issue {
    pub severity: Severity,
    pub message:  String,
}

impl Issue {
    pub(crate) fn error(msg: impl Into<String>) -> Self {
        Self { severity: Severity::Error, message: msg.into() }
    }

    pub(crate) fn warning(msg: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, message: msg.into() }
    }
}

/// Load and validate the fan config, printing results to stdout.
pub fn run() -> anyhow::Result<()> {
    let contents = match fs::read_to_string(CONFIG_PATH) {
        Ok(c) => c,
        Err(e) => {
            println!("error: cannot read {}: {}", CONFIG_PATH, e);
            std::process::exit(1);
        }
    };

    let config: FanConfig = match toml::from_str(&contents) {
        Ok(c) => c,
        Err(e) => {
            println!("error: failed to parse {}: {}", CONFIG_PATH, e);
            std::process::exit(1);
        }
    };

    let issues = validate(&config);

    let errors = issues.iter().filter(|i| i.severity == Severity::Error).count();
    let warnings = issues.iter().filter(|i| i.severity == Severity::Warning).count();

    for issue in &issues {
        let prefix = match issue.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        println!("{}: {}", prefix, issue.message);
    }

    if errors == 0 && warnings == 0 {
        println!("config ok");
    } else if errors == 0 {
        println!("\n{} warning(s), no errors", warnings);
    } else {
        println!("\n{} error(s), {} warning(s)", errors, warnings);
        std::process::exit(1);
    }

    Ok(())
}

/// Validate a parsed config and return any issues found.
pub(crate) fn validate(config: &FanConfig) -> Vec<Issue> {
    let mut issues = Vec::new();

    validate_curve(&config.curve, "shared", &mut issues);
    validate_critical_temps(config, &mut issues);
    validate_channels(config, &mut issues);
    validate_hwmon(config, &mut issues);

    issues
}

/// Check that curve points have strictly increasing temps and sane duty values.
fn validate_curve(points: &[fan::CurvePoint], label: &str, issues: &mut Vec<Issue>) {
    if points.is_empty() {
        issues.push(Issue::error(format!("{} curve has no points", label)));
        return;
    }

    for (i, point) in points.iter().enumerate() {
        if point.duty < 0.0 || point.duty > 100.0 {
            issues.push(Issue::error(format!(
                "{} curve point {}: duty {:.1} is outside 0-100 range",
                label, i, point.duty,
            )));
        }

        if i > 0 && point.temp <= points[i - 1].temp {
            issues.push(Issue::error(format!(
                "{} curve point {}: temp {:.1}C is not greater than previous {:.1}C",
                label, i, point.temp, points[i - 1].temp,
            )));
        }
    }
}

/// Check critical temperatures are in a reasonable range.
fn validate_critical_temps(config: &FanConfig, issues: &mut Vec<Issue>) {
    if config.critical_cpu_temp <= 0.0 || config.critical_cpu_temp > 120.0 {
        issues.push(Issue::error(format!(
            "critical_cpu_temp {:.1}C is outside reasonable range (0-120)",
            config.critical_cpu_temp,
        )));
    }

    if config.critical_gpu_temp <= 0.0 || config.critical_gpu_temp > 120.0 {
        issues.push(Issue::error(format!(
            "critical_gpu_temp {:.1}C is outside reasonable range (0-120)",
            config.critical_gpu_temp,
        )));
    }

    if let Some(h) = config.hysteresis {
        if !(0.0..=20.0).contains(&h) {
            issues.push(Issue::error(format!(
                "hysteresis {:.1}C is outside reasonable range (0-20)",
                h,
            )));
        }
    }
}

/// Validate channel configs, including per-channel curves.
fn validate_channels(config: &FanConfig, issues: &mut Vec<Issue>) {
    if config.channels.is_empty() {
        issues.push(Issue::error("no channels defined".to_string()));
    }

    for (i, ch) in config.channels.iter().enumerate() {
        let valid_sources = ["cpu", "gpu", "all"];
        if !valid_sources.contains(&ch.source.as_str()) {
            issues.push(Issue::warning(format!(
                "channel {} ({}): unknown source '{}', will default to 'all'",
                i, ch.pwm, ch.source,
            )));
        }

        if let Some(ref curve) = ch.curve {
            let label = format!("channel {} ({})", i, ch.pwm);
            validate_curve(curve, &label, issues);
        }
    }
}

/// Check that the platform hwmon and relevant temp sources exist on this machine.
fn validate_hwmon(config: &FanConfig, issues: &mut Vec<Issue>) {
    let hwmons = match HwMon::all() {
        Ok(h) => h,
        Err(_) => {
            issues.push(Issue::warning("could not enumerate hwmon devices".to_string()));
            return;
        }
    };

    let names: Vec<String> = hwmons
        .iter()
        .filter_map(|h| h.name().ok())
        .collect();

    if let Some(ref platform) = config.platform {
        if !names.iter().any(|n| n == platform) {
            issues.push(Issue::warning(format!(
                "platform '{}' not found in hwmon devices on this machine",
                platform,
            )));
        }
    }

    let has_cpu_hwmon = names.iter().any(|n| {
        matches!(n.as_str(), "coretemp" | "k10temp" | "zenpower" | "apm_xgene")
    });

    let needs_cpu = config.channels.iter().any(|ch| ch.source == "cpu" || ch.source == "all");
    if needs_cpu && !has_cpu_hwmon {
        issues.push(Issue::warning(
            "channels reference cpu temps but no cpu hwmon found (coretemp, k10temp, zenpower)".to_string(),
        ));
    }

    let has_gpu_hwmon = names.iter().any(|n| n == "amdgpu");
    let needs_gpu = config.channels.iter().any(|ch| ch.source == "gpu" || ch.source == "all");
    if needs_gpu && !has_gpu_hwmon {
        issues.push(Issue::warning(
            "channels reference gpu temps but no amdgpu hwmon found (NVIDIA uses NVML at runtime)".to_string(),
        ));
    }
}
