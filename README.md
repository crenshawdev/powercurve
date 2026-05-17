# powercurve

A Linux power management daemon with built-in fan curves.

## What it does

- Power profiles compatible with `power-profiles-daemon`. Desktop tooling
  and `powerprofilesctl` keep working unchanged.
- Configurable fan control through hwmon PWM with curves, hysteresis,
  and thermal protection.

Fan control is optional. Without a fan config, PowerCurve behaves like ppd.

**Desktop only.** This is built for desktops and workstations. Most
laptops manage fans through embedded controllers that don't expose PWM
via hwmon. Fan control won't work there. The power profile side
functions fine on any hardware.

## Installation

### Arch

Available on the [AUR](https://aur.archlinux.org/packages/powercurve-git):

```
yay -S powercurve-git
```

### Debian / Ubuntu / Pop!_OS

Download the latest `.deb` from the
[releases page](https://gitlab.com/vintagetechie/powercurve/-/releases):

```
sudo apt install ./powercurve_*.deb
```

Both packages provide and conflict with `power-profiles-daemon`. The
stock daemon gets replaced cleanly.

## Power profiles

Three profiles, switchable via D-Bus or the CLI:

- **Quiet** maps to `power-saver`. Conservative CPU governor, turbo
  disabled, aggressive PCI and SCSI power management.
- **Balanced** is the default. Dynamic CPU scaling with turbo enabled,
  moderate power management.
- **Performance** runs the CPU at full frequency with turbo and disables
  PCI runtime power management.

Set a profile:

```
powercurve profile quiet
powercurve profile balanced
powercurve profile performance
```

Query the current profile:

```
powercurve profile
```

The active profile persists across restarts.

## Fan control

Fan control reads `/etc/powercurve/fan.toml`. The Arch package generates
one on install by scanning hwmon devices. Without a config, the daemon
manages profiles only.

To inspect your hardware:

```
powercurve fan-detect
```

To generate a config (or regenerate after hardware changes):

```
powercurve fan-detect --generate | sudo tee /etc/powercurve/fan.toml > /dev/null
```

Critical temperatures get set 15C below the CPU's thermal throttle
point.

### A minimal config

```toml
platform = "nct6775"
critical_cpu_temp = 80
critical_gpu_temp = 75
hysteresis = 3.0

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

[[channels]]
pwm = "pwm1"
source = "cpu"

[[channels]]
pwm = "pwm2"
source = "all"
```

`platform` is the hwmon device with your PWM outputs (`fan-detect`
fills it in). The `source` on each channel picks the sensor: `cpu`,
`gpu`, or `all` (max of the two). Temperatures are Celsius, duties are
percentages. The daemon interpolates linearly between points.

Curves are layered: per-channel per-profile, per-channel default,
per-profile global, and a shared top-level fallback. First match wins.
The example above uses only the shared fallback. That covers most
setups.

The `examples/` directory has progressively more involved configs:
`fan-simple.toml`, `fan-desktop.toml`, `fan-profiles.toml`. Everything
else lives in `man powercurve`: minimum-duty floors, stall detection,
passthrough mode, thermal fallback, per-channel per-profile curves,
NVIDIA support via NVML, and the full config field reference.

## Status and overrides

Check the current state:

```
powercurve status
```

This shows the active profile, CPU and GPU temperatures, per-channel
PWM duties, RPMs, and the active curve for each channel. Doesn't need
root.

For testing, override a channel's duty without editing the config:

```
powercurve fan pwm3 50      # set pwm3 to 50%
powercurve fan pwm3 clear   # release
```

Switching profiles clears all overrides.

To find a channel's spin-up floor (the lowest duty where the fan
actually spins):

```
powercurve fan-test pwm1
```

Use the reported value as `min_duty` in `fan.toml`.

Desktop notifications on profile switches and thermal events come from
the monitor user service:

```
systemctl --user enable --now powercurve-monitor
```

## Process watcher

The watcher polls `/proc` and switches profiles automatically when a
process matches a rule. Useful for games and encoding jobs.

Configure it at `~/.config/powercurve/watcher.toml`:

```toml
[watcher]
poll_interval = 5
default_profile = "balanced"

[[rule]]
name = "gaming"
match_exe = "steam_app_*"
profile = "performance"

[[rule]]
name = "video-encoding"
match_cmd = "ffmpeg.*libx265"
profile = "performance"
```

`match_exe` is a glob on the process name. `match_cmd` is a regex on
the full command line. First match wins. When nothing matches, the
`default_profile` is restored if set.

Run as a user service:

```
systemctl --user enable --now powercurve-watcher
```

Without a config file, the watcher idles.

## Validation and hot reload

Validate without restarting:

```
powercurve config
```

This checks curve monotonicity, duty ranges, critical temp bounds,
hysteresis and cooldown values, profile names, per-channel profile
curves, and whether the referenced hwmon devices exist on the current
machine.

Reload after editing:

```
sudo systemctl reload powercurve
```

The daemon validates first. If validation fails, the running config is
kept and the error is logged.

## Building

Requires a stable Rust toolchain:

```
make
sudo make install
sudo systemctl enable --now com.vintagetechie.PowerCurve
```

## Use at your own risk

This daemon writes directly to hardware PWM registers. A bad fan config
can result in inadequate cooling and hardware damage. The built-in
thermal protection sets fans to maximum when critical temperatures are
reached, but it is a safety net, not a substitute for a correct
configuration. Test your curves, verify your temps, and don't deploy a
config you haven't validated. See the LICENSE for full terms.

## More

`man powercurve` is the full reference. Every command, every config
option, the D-Bus interface, signals, and file locations.

[CHANGELOG.md](CHANGELOG.md) lists what changed in each release.
[ROLLBACK.md](ROLLBACK.md) covers switching back to
`power-profiles-daemon` or downgrading to a previous version.

## License

GPL-3.0-only

Based on [system76-power](https://github.com/pop-os/system76-power) by
System76, also licensed under GPL-3.0-only.
