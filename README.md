# vintagetechie-power

A lightweight power management daemon for Linux desktops. Drop-in
replacement for `power-profiles-daemon` with deeper hardware control
and configurable fan curves.

Profiles adjust CPU governors, turbo boost, PCI runtime power management,
SCSI link policies, and ACPI platform profiles. Desktop environments
that talk to the `org.freedesktop.UPower.PowerProfiles` D-Bus interface
(GNOME, KDE, `powerprofilesctl`) pick up profiles automatically.

## Installation

Available from the [VintageTechie Arch repo](https://vintagetechie.codeberg.page/vintagetechie-arch-repo/):

```
sudo pacman -S vintagetechie-power-git
```

The package provides and conflicts with `power-profiles-daemon`, so
it replaces the stock daemon cleanly.

## Power profiles

Three profiles, switchable via D-Bus or the CLI:

- **Quiet** maps to `power-saver`. Conservative CPU governor (powersave,
  50% max frequency), turbo disabled, aggressive power management on
  PCI and SCSI devices.
- **Balanced** is the default. Dynamic CPU scaling with turbo enabled,
  moderate power management.
- **Performance** runs the CPU at full frequency with turbo, disables
  PCI runtime power management for maximum throughput.

Set a profile:

```
vintagetechie-power profile quiet
vintagetechie-power profile balanced
vintagetechie-power profile performance
```

Query the current profile:

```
vintagetechie-power profile
```

The active profile persists across restarts.

## Fan control

The daemon controls fans through hwmon PWM outputs using a config file
at `/etc/vintagetechie-power/fan.toml`. The Arch package generates this
automatically on install by scanning hwmon devices for PWM-capable
controllers. Without a config, fan control is disabled and the daemon
only manages power profiles.

Run `fan-detect` to see what the daemon found on your hardware:

```
vintagetechie-power fan-detect
```

This prints every hwmon device on the system with its temperature
sensors, PWM outputs, fan RPMs, and labels, then suggests a config.

To regenerate the config (or create one if it wasn't generated at
install time):

```
sudo vintagetechie-power fan-detect --generate > /etc/vintagetechie-power/fan.toml
```

The `--generate` flag outputs only the TOML config with no device
summary, making it safe to pipe directly to the config path.

### Config format

Each channel maps a PWM output to a temperature source (`cpu`, `gpu`,
or `all`) and follows a fan curve. Temperatures are in Celsius, duty is
a percentage (0-100). Channels without their own curve use the shared
top-level curve.

```toml
# hwmon device that controls the fans (find yours with fan-detect)
platform = "nct6775"

# all fans go to max when either threshold is hit
critical_cpu_temp = 80
critical_gpu_temp = 75

# shared fan curve
[[curve]]
temp = 30.0
duty = 10

[[curve]]
temp = 50.0
duty = 30

[[curve]]
temp = 70.0
duty = 80

[[curve]]
temp = 75.0
duty = 100

# channel mapping
[[channels]]
pwm = "pwm1"
source = "cpu"

[[channels]]
pwm = "pwm2"
source = "all"

[[channels]]
pwm = "pwm3"
source = "gpu"
```

Channels can override the shared curve with their own:

```toml
[[channels]]
pwm = "pwm3"
source = "gpu"

  [[channels.curve]]
  temp = 40.0
  duty = 0

  [[channels.curve]]
  temp = 80.0
  duty = 100
```

The `platform` field tells the daemon which hwmon device has your fan
PWM outputs. Common values: `nct6775`, `it8688`, `asus-ec-sensors`.
`fan-detect` fills this in automatically based on what it finds.

### Temperature sources

CPU temps come from hwmon drivers (`coretemp`, `k10temp`, `zenpower`).
GPU temps combine two sources: AMD GPUs via the `amdgpu` hwmon driver
and NVIDIA GPUs via NVML (loaded at runtime from `libnvidia-ml.so.1`,
no dependency on `nvidia-smi`). If NVIDIA hardware is detected but NVML
can't load, GPU-sourced channels run at max duty as a safety measure.

If any sensor crosses its critical threshold, all fans go to maximum
regardless of the curve.

## Building

Requires a stable Rust toolchain:

```
make
sudo make install
```

The daemon runs as a systemd service:

```
sudo systemctl enable --now com.vintagetechie.PowerDaemon
```

## License

GPL-3.0-only

Based on [system76-power](https://github.com/pop-os/system76-power)
by System76, also licensed under GPL-3.0-only.
