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
    pub message: String,
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
    validate_profiles(config, &mut issues);
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
                label,
                i,
                point.temp,
                points[i - 1].temp,
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

    if let Some(h) = config.hysteresis
        && !(0.0..=20.0).contains(&h)
    {
        issues
            .push(Issue::error(
                format!("hysteresis {:.1}C is outside reasonable range (0-20)", h,),
            ));
    }

    if let Some(cd) = config.thermal_cooldown
        && (cd == 0 || cd > 300)
    {
        issues.push(Issue::error(format!(
            "thermal_cooldown {}s is outside reasonable range (1-300)",
            cd,
        )));
    }
}

/// Validate channel configs, including per-channel curves.
fn validate_channels(config: &FanConfig, issues: &mut Vec<Issue>) {
    if config.channels.is_empty() {
        issues.push(Issue::error("no channels defined".to_string()));
    }

    for (i, ch) in config.channels.iter().enumerate() {
        if ch.passthrough == Some(true) {
            if ch.curve.is_some()
                || ch.profiles.is_some()
                || ch.min_duty.is_some()
                || ch.stall_detect.is_some()
            {
                issues.push(Issue::warning(format!(
                    "channel {} ({}): passthrough is set, curve/profile/min_duty/stall settings are ignored",
                    i, ch.pwm,
                )));
            }
            continue;
        }

        let valid_sources = ["cpu", "gpu", "all"];
        if !valid_sources.contains(&ch.source.as_str()) {
            issues.push(Issue::warning(format!(
                "channel {} ({}): unknown source '{}', will default to 'all'",
                i, ch.pwm, ch.source,
            )));
        }

        if let Some(min) = ch.min_duty
            && !(0.0..=100.0).contains(&min)
        {
            issues.push(Issue::error(format!(
                "channel {} ({}): min_duty {:.1} is outside 0-100 range",
                i, ch.pwm, min,
            )));
        }

        if let Some(t) = ch.stall_threshold
            && (t == 0 || t > 10)
        {
            issues.push(Issue::error(format!(
                "channel {} ({}): stall_threshold {} is outside reasonable range (1-10)",
                i, ch.pwm, t,
            )));
        }

        if let Some(ref curve) = ch.curve {
            let label = format!("channel {} ({})", i, ch.pwm);
            validate_curve(curve, &label, issues);
        }

        if let Some(ref profiles) = ch.profiles {
            let valid_names = ["quiet", "balanced", "performance"];
            for (name, profile) in profiles {
                if !valid_names.contains(&name.to_lowercase().as_str()) {
                    issues.push(Issue::warning(format!(
                        "channel {} ({}) profile '{}': unknown profile name",
                        i, ch.pwm, name,
                    )));
                }

                let label = format!("channel {} ({}) profile '{}'", i, ch.pwm, name);
                validate_curve(&profile.curve, &label, issues);
            }
        }
    }
}

/// Validate per-profile curve overrides.
fn validate_profiles(config: &FanConfig, issues: &mut Vec<Issue>) {
    let Some(ref profiles) = config.profiles else { return };

    let valid_names = ["quiet", "balanced", "performance"];
    for (name, profile) in profiles {
        if !valid_names.contains(&name.to_lowercase().as_str()) {
            issues.push(Issue::warning(format!(
                "profile '{}': unknown profile name, expected one of: quiet, balanced, performance",
                name,
            )));
        }

        let label = format!("profile '{}'", name);
        validate_curve(&profile.curve, &label, issues);
    }
}

/// Check that the platform hwmon and relevant temp sources exist on this machine.
/// Skipped in tests since hwmon availability depends on the running machine.
fn validate_hwmon(config: &FanConfig, issues: &mut Vec<Issue>) {
    let hwmons = match HwMon::all() {
        Ok(h) => h,
        Err(_) => {
            issues.push(Issue::warning("could not enumerate hwmon devices".to_string()));
            return;
        }
    };

    let names: Vec<String> = hwmons.iter().filter_map(|h| h.name().ok()).collect();

    if let Some(ref platform) = config.platform
        && !names.iter().any(|n| n == platform)
    {
        issues.push(Issue::warning(format!(
            "platform '{}' not found in hwmon devices on this machine",
            platform,
        )));
    }

    let has_cpu_hwmon = names
        .iter()
        .any(|n| matches!(n.as_str(), "coretemp" | "k10temp" | "zenpower" | "apm_xgene"));

    let needs_cpu = config.channels.iter().any(|ch| ch.source == "cpu" || ch.source == "all");
    if needs_cpu && !has_cpu_hwmon {
        issues.push(Issue::warning(
            "channels reference cpu temps but no cpu hwmon found (coretemp, k10temp, zenpower)"
                .to_string(),
        ));
    }

    let has_gpu_hwmon = names.iter().any(|n| n == "amdgpu");
    let needs_gpu = config.channels.iter().any(|ch| ch.source == "gpu" || ch.source == "all");
    if needs_gpu && !has_gpu_hwmon {
        issues.push(Issue::warning(
            "channels reference gpu temps but no amdgpu hwmon found (NVIDIA uses NVML at runtime)"
                .to_string(),
        ));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::fan::{ChannelConfig, ChannelProfileConfig, CurvePoint, FanConfig, ProfileConfig};
    use std::collections::HashMap;

    /// Build a minimal valid config for testing.
    fn valid_config() -> FanConfig {
        FanConfig {
            platform: None,
            critical_cpu_temp: 80.0,
            critical_gpu_temp: 75.0,
            hysteresis: None,
            thermal_fallback: None,
            thermal_cooldown: None,
            curve: vec![
                CurvePoint { temp: 30.0, duty: 10.0 },
                CurvePoint { temp: 50.0, duty: 50.0 },
                CurvePoint { temp: 75.0, duty: 100.0 },
            ],
            channels: vec![ChannelConfig {
                pwm: "pwm1".into(),
                source: "cpu".into(),
                min_duty: None,
                stall_detect: None,
                stall_threshold: None,
                passthrough: None,
                curve: None,
                profiles: None,
            }],
            profiles: None,
        }
    }

    /// Filter issues to just errors or just warnings.
    fn errors(issues: &[Issue]) -> Vec<&str> {
        issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .map(|i| i.message.as_str())
            .collect()
    }

    fn warnings(issues: &[Issue]) -> Vec<&str> {
        issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .map(|i| i.message.as_str())
            .collect()
    }

    #[test]
    fn valid_config_no_issues() {
        let config = valid_config();
        let mut issues = Vec::new();
        validate_curve(&config.curve, "shared", &mut issues);
        validate_critical_temps(&config, &mut issues);
        validate_channels(&config, &mut issues);
        validate_profiles(&config, &mut issues);
        assert!(errors(&issues).is_empty());
        assert!(warnings(&issues).is_empty());
    }

    #[test]
    fn curve_not_monotonic() {
        let mut config = valid_config();
        config.curve =
            vec![CurvePoint { temp: 50.0, duty: 50.0 }, CurvePoint { temp: 30.0, duty: 10.0 }];
        let mut issues = Vec::new();
        validate_curve(&config.curve, "shared", &mut issues);
        assert_eq!(errors(&issues).len(), 1);
        assert!(errors(&issues)[0].contains("not greater than previous"));
    }

    #[test]
    fn duty_out_of_range() {
        let mut config = valid_config();
        config.curve =
            vec![CurvePoint { temp: 30.0, duty: -5.0 }, CurvePoint { temp: 50.0, duty: 110.0 }];
        let mut issues = Vec::new();
        validate_curve(&config.curve, "shared", &mut issues);
        assert_eq!(errors(&issues).len(), 2);
    }

    #[test]
    fn critical_temp_out_of_range() {
        let mut config = valid_config();
        config.critical_cpu_temp = 150.0;
        config.critical_gpu_temp = -10.0;
        let mut issues = Vec::new();
        validate_critical_temps(&config, &mut issues);
        assert_eq!(errors(&issues).len(), 2);
    }

    #[test]
    fn unknown_profile_name() {
        let mut config = valid_config();
        let mut profiles = HashMap::new();
        profiles.insert(
            "turbo".into(),
            ProfileConfig { curve: vec![CurvePoint { temp: 30.0, duty: 80.0 }] },
        );
        config.profiles = Some(profiles);
        let mut issues = Vec::new();
        validate_profiles(&config, &mut issues);
        assert_eq!(warnings(&issues).len(), 1);
        assert!(warnings(&issues)[0].contains("unknown profile name"));
    }

    #[test]
    fn thermal_cooldown_out_of_range() {
        let mut config = valid_config();
        config.thermal_cooldown = Some(0);
        let mut issues = Vec::new();
        validate_critical_temps(&config, &mut issues);
        assert_eq!(errors(&issues).len(), 1);
        assert!(errors(&issues)[0].contains("thermal_cooldown"));

        config.thermal_cooldown = Some(500);
        issues.clear();
        validate_critical_temps(&config, &mut issues);
        assert_eq!(errors(&issues).len(), 1);
    }

    #[test]
    fn hysteresis_out_of_range() {
        let mut config = valid_config();
        config.hysteresis = Some(25.0);
        let mut issues = Vec::new();
        validate_critical_temps(&config, &mut issues);
        assert_eq!(errors(&issues).len(), 1);
        assert!(errors(&issues)[0].contains("hysteresis"));
    }

    #[test]
    fn no_channels_is_error() {
        let mut config = valid_config();
        config.channels.clear();
        let mut issues = Vec::new();
        validate_channels(&config, &mut issues);
        assert_eq!(errors(&issues).len(), 1);
        assert!(errors(&issues)[0].contains("no channels"));
    }

    #[test]
    fn unknown_source_is_warning() {
        let mut config = valid_config();
        config.channels = vec![ChannelConfig {
            pwm: "pwm1".into(),
            source: "memory".into(),
            min_duty: None,
            stall_detect: None,
            stall_threshold: None,
            passthrough: None,
            curve: None,
            profiles: None,
        }];
        let mut issues = Vec::new();
        validate_channels(&config, &mut issues);
        assert_eq!(warnings(&issues).len(), 1);
        assert!(warnings(&issues)[0].contains("unknown source"));
    }

    #[test]
    fn channel_profile_unknown_name_is_warning() {
        let mut config = valid_config();
        let mut profiles = HashMap::new();
        profiles.insert(
            "turbo".into(),
            ChannelProfileConfig { curve: vec![CurvePoint { temp: 30.0, duty: 10.0 }] },
        );
        config.channels = vec![ChannelConfig {
            pwm: "pwm1".into(),
            source: "cpu".into(),
            min_duty: None,
            stall_detect: None,
            stall_threshold: None,
            passthrough: None,
            curve: None,
            profiles: Some(profiles),
        }];
        let mut issues = Vec::new();
        validate_channels(&config, &mut issues);
        assert_eq!(warnings(&issues).len(), 1);
        assert!(warnings(&issues)[0].contains("unknown profile name"));
    }

    #[test]
    fn min_duty_out_of_range() {
        let mut config = valid_config();
        config.channels = vec![ChannelConfig {
            pwm: "pwm1".into(),
            source: "cpu".into(),
            min_duty: Some(150.0),
            stall_detect: None,
            stall_threshold: None,
            passthrough: None,
            curve: None,
            profiles: None,
        }];
        let mut issues = Vec::new();
        validate_channels(&config, &mut issues);
        assert_eq!(errors(&issues).len(), 1);
        assert!(errors(&issues)[0].contains("min_duty"));
    }

    #[test]
    fn min_duty_valid() {
        let mut config = valid_config();
        config.channels = vec![ChannelConfig {
            pwm: "pwm1".into(),
            source: "cpu".into(),
            min_duty: Some(15.0),
            stall_detect: None,
            stall_threshold: None,
            passthrough: None,
            curve: None,
            profiles: None,
        }];
        let mut issues = Vec::new();
        validate_channels(&config, &mut issues);
        assert!(errors(&issues).is_empty());
    }

    #[test]
    fn channel_profile_bad_curve_is_error() {
        let mut config = valid_config();
        let mut profiles = HashMap::new();
        profiles.insert(
            "quiet".into(),
            ChannelProfileConfig {
                curve: vec![
                    CurvePoint { temp: 50.0, duty: 50.0 },
                    CurvePoint { temp: 30.0, duty: 10.0 },
                ],
            },
        );
        config.channels = vec![ChannelConfig {
            pwm: "pwm1".into(),
            source: "cpu".into(),
            min_duty: None,
            stall_detect: None,
            stall_threshold: None,
            passthrough: None,
            curve: None,
            profiles: Some(profiles),
        }];
        let mut issues = Vec::new();
        validate_channels(&config, &mut issues);
        assert_eq!(errors(&issues).len(), 1);
        assert!(errors(&issues)[0].contains("not greater than previous"));
    }

    #[test]
    fn stall_threshold_out_of_range() {
        let mut config = valid_config();
        config.channels = vec![ChannelConfig {
            pwm: "pwm1".into(),
            source: "cpu".into(),
            min_duty: None,
            stall_detect: Some(true),
            stall_threshold: Some(0),
            passthrough: None,
            curve: None,
            profiles: None,
        }];
        let mut issues = Vec::new();
        validate_channels(&config, &mut issues);
        assert_eq!(errors(&issues).len(), 1);
        assert!(errors(&issues)[0].contains("stall_threshold"));

        config.channels[0].stall_threshold = Some(15);
        issues.clear();
        validate_channels(&config, &mut issues);
        assert_eq!(errors(&issues).len(), 1);
    }

    #[test]
    fn stall_threshold_valid() {
        let mut config = valid_config();
        config.channels = vec![ChannelConfig {
            pwm: "pwm1".into(),
            source: "cpu".into(),
            min_duty: None,
            stall_detect: Some(true),
            stall_threshold: Some(5),
            passthrough: None,
            curve: None,
            profiles: None,
        }];
        let mut issues = Vec::new();
        validate_channels(&config, &mut issues);
        assert!(errors(&issues).is_empty());
    }

    #[test]
    fn passthrough_skips_validation() {
        let mut config = valid_config();
        config.channels = vec![ChannelConfig {
            pwm: "pwm4".into(),
            source: "all".into(),
            min_duty: None,
            stall_detect: None,
            stall_threshold: None,
            passthrough: Some(true),
            curve: None,
            profiles: None,
        }];
        let mut issues = Vec::new();
        validate_channels(&config, &mut issues);
        assert!(errors(&issues).is_empty());
        assert!(warnings(&issues).is_empty());
    }

    #[test]
    fn passthrough_warns_on_ignored_settings() {
        let mut config = valid_config();
        config.channels = vec![ChannelConfig {
            pwm: "pwm4".into(),
            source: "all".into(),
            min_duty: Some(25.0),
            stall_detect: None,
            stall_threshold: None,
            passthrough: Some(true),
            curve: None,
            profiles: None,
        }];
        let mut issues = Vec::new();
        validate_channels(&config, &mut issues);
        assert_eq!(warnings(&issues).len(), 1);
        assert!(warnings(&issues)[0].contains("passthrough"));
    }
}
