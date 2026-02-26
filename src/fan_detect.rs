// SPDX-License-Identifier: GPL-3.0-only

use std::{fmt::Write, fs, path::Path};

/// Known CPU temperature hwmon drivers.
const CPU_DRIVERS: &[&str] = &["coretemp", "k10temp", "zenpower", "apm_xgene"];

/// Known GPU temperature hwmon drivers.
const GPU_DRIVERS: &[&str] = &["amdgpu"];

/// A discovered hwmon device with its temperature and PWM capabilities.
struct HwmonDevice {
    index: String,
    name:  String,
    temps: Vec<TempInput>,
    pwms:  Vec<PwmOutput>,
}

struct TempInput {
    file:  String,
    label: String,
    value: String,
}

struct PwmOutput {
    file:   String,
    value:  String,
    max:    String,
    rpm:    Option<String>,
    label:  String,
}

/// Enumerate hwmon devices and print a summary plus a starter fan.toml.
///
/// When `generate` is true, outputs only the TOML config with no device
/// summary. Useful for piping directly to a config file.
pub fn run(generate: bool) -> anyhow::Result<()> {
    let devices = discover_hwmon()?;

    if devices.is_empty() {
        if !generate {
            println!("No hwmon devices found.");
        }
        return Ok(());
    }

    // Pick the best platform candidate: first device with PWM outputs
    // that isn't a known CPU or GPU driver.
    let platform = devices.iter().find(|d| {
        !d.pwms.is_empty()
            && !CPU_DRIVERS.contains(&d.name.as_str())
            && !GPU_DRIVERS.contains(&d.name.as_str())
    });

    if generate {
        if let Some(plat) = platform {
            print!("{}", generate_config(plat));
        }
        return Ok(());
    }

    println!("Found hwmon devices:\n");
    for dev in &devices {
        print_device(dev);
    }

    if let Some(plat) = platform {
        println!("\nSuggested fan.toml:\n");
        print!("{}", generate_config(plat));
    } else {
        println!("\nNo hwmon device with PWM outputs found for fan control.");
        println!("If your fans are controlled through a different interface,");
        println!("check `cat /sys/class/hwmon/hwmon*/name` for available devices.");
    }

    Ok(())
}

/// Walk /sys/class/hwmon and collect device info.
fn discover_hwmon() -> anyhow::Result<Vec<HwmonDevice>> {
    let hwmon_dir = Path::new("/sys/class/hwmon");
    if !hwmon_dir.exists() {
        return Ok(Vec::new());
    }

    let mut devices = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(hwmon_dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let index = entry.file_name().to_string_lossy().into_owned();

        let name = match fs::read_to_string(path.join("name")) {
            Ok(n) => n.trim().to_owned(),
            Err(_) => continue,
        };

        let temps = discover_temps(&path);
        let pwms = discover_pwms(&path);

        devices.push(HwmonDevice { index, name, temps, pwms });
    }

    Ok(devices)
}

/// Find all tempN_input files and their labels/values.
fn discover_temps(hwmon_path: &Path) -> Vec<TempInput> {
    let mut temps = Vec::new();

    for i in 1..=16 {
        let input_file = format!("temp{}_input", i);
        let input_path = hwmon_path.join(&input_file);
        if !input_path.exists() {
            continue;
        }

        let value = fs::read_to_string(&input_path)
            .map(|v| v.trim().to_owned())
            .unwrap_or_default();

        let label_file = format!("temp{}_label", i);
        let label = fs::read_to_string(hwmon_path.join(label_file))
            .map(|l| l.trim().to_owned())
            .unwrap_or_default();

        temps.push(TempInput { file: input_file, label, value });
    }

    temps
}

/// Find all pwmN files, their current values, and any matching fan info.
fn discover_pwms(hwmon_path: &Path) -> Vec<PwmOutput> {
    let mut pwms = Vec::new();

    for i in 1..=8 {
        let pwm_file = format!("pwm{}", i);
        let pwm_path = hwmon_path.join(&pwm_file);
        if !pwm_path.exists() {
            continue;
        }

        let value = fs::read_to_string(&pwm_path)
            .map(|v| v.trim().to_owned())
            .unwrap_or_else(|_| "?".into());

        let max = fs::read_to_string(hwmon_path.join(format!("pwm{}_max", i)))
            .map(|v| v.trim().to_owned())
            .unwrap_or_else(|_| "255".into());

        // Look for a matching fanN_input and fanN_label
        let rpm = fs::read_to_string(hwmon_path.join(format!("fan{}_input", i)))
            .map(|v| v.trim().to_owned())
            .ok();

        let label = fs::read_to_string(hwmon_path.join(format!("fan{}_label", i)))
            .map(|l| l.trim().to_owned())
            .unwrap_or_default();

        pwms.push(PwmOutput { file: pwm_file, value, max, rpm, label });
    }

    pwms
}

/// Print a single hwmon device summary.
fn print_device(dev: &HwmonDevice) {
    println!("  {}: {}", dev.index, dev.name);

    for temp in &dev.temps {
        let millideg: f64 = temp.value.parse().unwrap_or(0.0);
        let celsius = millideg / 1000.0;
        if temp.label.is_empty() {
            println!("    {}: {:.1}C", temp.file, celsius);
        } else {
            println!("    {}: {} ({:.1}C)", temp.file, temp.label, celsius);
        }
    }

    for pwm in &dev.pwms {
        let rpm_str = pwm.rpm.as_deref().unwrap_or("-");
        if pwm.label.is_empty() {
            println!("    {}: {}/{} ({} rpm)", pwm.file, pwm.value, pwm.max, rpm_str);
        } else {
            println!(
                "    {}: {}/{} ({} rpm) [{}]",
                pwm.file, pwm.value, pwm.max, rpm_str, pwm.label
            );
        }
    }

    println!();
}

/// Guess a temperature source from a fan label. Falls back to "all"
/// when the label doesn't contain a recognizable keyword.
fn source_from_label(label: &str) -> &'static str {
    let lower = label.to_lowercase();
    if lower.contains("cpu") {
        "cpu"
    } else if lower.contains("gpu") {
        "gpu"
    } else {
        "all"
    }
}

/// Generate a starter fan.toml from a platform device.
fn generate_config(platform: &HwmonDevice) -> String {
    let mut out = String::new();
    let has_labels = platform.pwms.iter().any(|p| !p.label.is_empty());

    writeln!(out, "platform = \"{}\"", platform.name).ok();
    writeln!(out, "critical_cpu_temp = 80").ok();
    writeln!(out, "critical_gpu_temp = 75").ok();
    writeln!(out).ok();
    writeln!(out, "# Smooth fan curve with tight steps through the idle range and a").ok();
    writeln!(out, "# gentle ramp into load. Always-on floor at 10% avoids start/stop cycling.").ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 30.0").ok();
    writeln!(out, "duty = 10").ok();
    writeln!(out).ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 36.0").ok();
    writeln!(out, "duty = 12").ok();
    writeln!(out).ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 40.0").ok();
    writeln!(out, "duty = 18").ok();
    writeln!(out).ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 43.0").ok();
    writeln!(out, "duty = 22").ok();
    writeln!(out).ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 46.0").ok();
    writeln!(out, "duty = 26").ok();
    writeln!(out).ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 50.0").ok();
    writeln!(out, "duty = 30").ok();
    writeln!(out).ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 54.0").ok();
    writeln!(out, "duty = 38").ok();
    writeln!(out).ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 58.0").ok();
    writeln!(out, "duty = 45").ok();
    writeln!(out).ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 62.0").ok();
    writeln!(out, "duty = 55").ok();
    writeln!(out).ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 66.0").ok();
    writeln!(out, "duty = 68").ok();
    writeln!(out).ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 70.0").ok();
    writeln!(out, "duty = 80").ok();
    writeln!(out).ok();
    writeln!(out, "[[curve]]").ok();
    writeln!(out, "temp = 75.0").ok();
    writeln!(out, "duty = 100").ok();

    for pwm in &platform.pwms {
        writeln!(out).ok();
        if !pwm.label.is_empty() {
            writeln!(out, "# {}", pwm.label).ok();
        }
        writeln!(out, "[[channels]]").ok();
        writeln!(out, "pwm = \"{}\"", pwm.file).ok();
        let source = if has_labels {
            source_from_label(&pwm.label)
        } else {
            "all"
        };
        writeln!(out, "source = \"{}\"", source).ok();
    }

    if !has_labels {
        writeln!(out).ok();
        writeln!(out, "# No fan labels found. All channels default to source = \"all\".").ok();
        writeln!(out, "# Adjust sources to \"cpu\" or \"gpu\" based on your fan layout.").ok();
    }

    out
}
