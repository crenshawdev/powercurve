# vintagetechie-power

> **Work in progress.** Power profile switching works on any Linux desktop.
> Fan control currently requires a config file and a compatible hwmon
> controller. See the roadmap below for what's coming.

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

Fan control is optional. Without a config file, the daemon handles power
profiles only. To enable fan curves, create `/etc/vintagetechie-power/fan.toml`.

Each channel maps a PWM output to a temperature source (`cpu`, `gpu`,
or `all`) and follows a shared fan curve. Temperatures are in Celsius,
duty is a percentage (0-100).

```toml
# hwmon device that controls the fans
platform = "nct6775"

# all fans go to max when either threshold is hit
critical_cpu_temp = 85
critical_gpu_temp = 80

# shared fan curve
[[curve]]
temp = 35.0
duty = 0

[[curve]]
temp = 50.0
duty = 40

[[curve]]
temp = 65.0
duty = 75

[[curve]]
temp = 75.0
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
```

The `platform` field tells the daemon which hwmon device has your fan
PWM outputs. Common values: `nct6775`, `it8688`, `asus-ec-sensors`.
Check what's on your system with `cat /sys/class/hwmon/hwmon*/name`.

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

## Roadmap

The fan daemon is being reworked to support any Linux desktop with
hwmon-based fan control. Planned changes:

- `fan-detect` CLI command to enumerate hwmon devices and generate a
  starter config
- Per-channel fan curve overrides (different curves for CPU fan vs.
  case fans)
- Generic hwmon platform discovery (currently requires manual config
  on non-System76 hardware)

## License

GPL-3.0-only

Based on [system76-power](https://github.com/pop-os/system76-power)
by System76, also licensed under GPL-3.0-only.
