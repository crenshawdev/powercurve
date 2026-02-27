# powercurve

A lightweight power management daemon for Linux desktops. Drop-in
replacement for `power-profiles-daemon` with deeper hardware control,
configurable fan curves, and thermal protection.

Profiles adjust CPU governors, turbo boost, PCI runtime power management,
SCSI link policies, and ACPI platform profiles. Desktop environments
that talk to the `org.freedesktop.UPower.PowerProfiles` D-Bus interface
(GNOME, KDE, `powerprofilesctl`) pick up profiles automatically.

Fan control runs through hwmon PWM outputs with a layered curve system
that lets you tune each fan independently per profile. The daemon
interpolates between curve points, applies hysteresis to prevent
flapping, and handles thermal emergencies when temps get critical.

**Desktop only.** This is built for desktops and workstations. Most
laptops manage fans through embedded controllers that don't expose
PWM via hwmon, so fan control won't work. The power profile side
functions fine on any hardware, but if fan curves are what you're
after, stick with `power-profiles-daemon` on laptops.

## Installation

### Arch

Available from the [VintageTechie Arch repo](https://vintagetechie.codeberg.page/vintagetechie-arch-repo/):

```
sudo pacman -S powercurve-git
```

### Debian / Ubuntu / Pop!_OS

Download the latest `.deb` from the
[releases page](https://codeberg.org/VintageTechie/powercurve/releases):

```
sudo apt install ./powercurve_*.deb
```

Both packages provide and conflict with `power-profiles-daemon`, so
the stock daemon gets replaced cleanly.

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

The daemon controls fans through hwmon PWM outputs using a config file
at `/etc/powercurve/fan.toml`. The Arch package generates this
automatically on install by scanning hwmon devices for PWM-capable
controllers. Without a config, fan control is disabled and the daemon
only manages power profiles.

Run `fan-detect` to see what the daemon found on your hardware:

```
powercurve fan-detect
```

This prints every hwmon device on the system with its temperature
sensors, PWM outputs, fan RPMs, labels, and critical temp thresholds,
then suggests a config with auto-detected critical temperatures.

To generate a config (or regenerate after hardware changes):

```
sudo powercurve fan-detect --generate > /etc/powercurve/fan.toml
```

The `--generate` flag outputs only the TOML config with no device
summary, making it safe to pipe directly to the config path. Critical
temperatures are set 15C below the CPU's thermal throttle point
(read from sysfs when available, with per-driver fallbacks for chips
that don't expose it).

### Config format

The config has four layers of fan curves, from broadest to most
specific. When the daemon picks a curve for a channel, it walks
this list and uses the first match:

1. **Per-channel per-profile** - this specific fan on this specific profile
2. **Per-channel default** - this fan's curve regardless of profile
3. **Per-profile global** - all fans without overrides on this profile
4. **Shared top-level** - the fallback for everything else

You only need layers 4 and 1 (shared curve + channels) to get started.
The rest is there when you want finer control.

Here's a config that uses all four layers:

```toml
platform = "nct6775"
critical_cpu_temp = 80
critical_gpu_temp = 75
hysteresis = 3.0
thermal_fallback = true
thermal_cooldown = 30

# Layer 4: shared curve (fallback for all channels)
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

# CPU fan, follows the shared/profile curve
[[channels]]
pwm = "pwm1"
source = "cpu"

# Case fan, responds to the hottest source
[[channels]]
pwm = "pwm2"
source = "all"

# GPU fan with its own default curve (layer 2)
# and per-profile overrides (layer 1)
[[channels]]
pwm = "pwm3"
source = "gpu"
curve = [
  { temp = 30.0, duty = 5.0 },
  { temp = 55.0, duty = 20.0 },
  { temp = 68.0, duty = 65.0 },
  { temp = 80.0, duty = 100.0 },
]

[channels.profiles.quiet]
curve = [
  { temp = 30.0, duty = 3.0 },
  { temp = 60.0, duty = 15.0 },
  { temp = 74.0, duty = 70.0 },
  { temp = 80.0, duty = 100.0 },
]

[channels.profiles.performance]
curve = [
  { temp = 30.0, duty = 8.0 },
  { temp = 55.0, duty = 35.0 },
  { temp = 68.0, duty = 80.0 },
  { temp = 80.0, duty = 100.0 },
]

# Layer 3: global profile curves (all channels without overrides)
[profiles.quiet]
curve = [
  { temp = 30.0, duty = 5.0 },
  { temp = 50.0, duty = 14.0 },
  { temp = 72.0, duty = 65.0 },
  { temp = 80.0, duty = 100.0 },
]

[profiles.performance]
curve = [
  { temp = 30.0, duty = 15.0 },
  { temp = 48.0, duty = 38.0 },
  { temp = 70.0, duty = 90.0 },
  { temp = 78.0, duty = 100.0 },
]
```

The `platform` field tells the daemon which hwmon device has your fan
PWM outputs. Common values: `nct6775`, `it8688`, `asus-ec-sensors`.
`fan-detect` fills this in automatically based on what it finds.

Temperatures are in Celsius, duty is a percentage (0-100). The daemon
interpolates linearly between curve points, so you don't need a point
at every degree.

### Config reference

| Field | Required | Description |
|---|---|---|
| `platform` | no | hwmon device name for PWM outputs |
| `critical_cpu_temp` | yes | all fans go to max above this (Celsius) |
| `critical_gpu_temp` | yes | all fans go to max above this (Celsius) |
| `hysteresis` | no | temp must drop this many degrees before fans slow down (default 3.0) |
| `thermal_fallback` | no | auto-downshift profile on critical temps (default false) |
| `thermal_cooldown` | no | seconds to wait before restoring profile after thermal fallback (default 30) |

### Temperature sources

Each channel's `source` field controls which sensors drive it:

- `cpu` reads from hwmon drivers: `coretemp` (Intel), `k10temp` or `zenpower` (AMD)
- `gpu` combines AMD GPUs via the `amdgpu` hwmon driver and NVIDIA GPUs
  via NVML (loaded at runtime from `libnvidia-ml.so.1`, no dependency
  on `nvidia-smi`)
- `all` takes the max of CPU and GPU temps

If NVIDIA hardware is detected but NVML can't load, GPU-sourced
channels run at max duty as a safety measure.

### Thermal protection

When any sensor crosses its critical threshold, all fans immediately
go to maximum regardless of the curve. With `thermal_fallback = true`,
the daemon also downshifts the power profile (Performance to Balanced,
Balanced to Quiet) to reduce heat generation. Once temps stabilize
for `thermal_cooldown` seconds, the original profile is restored.

### Hysteresis

Fan curves apply hysteresis to prevent rapid cycling. When temps are
falling, the fan holds its current duty until the temperature drops
by the configured hysteresis amount (default 3C). Rising temps always
update immediately. This keeps fans from bouncing between speeds when
the CPU is hovering around a curve point.

### Minimum duty floor

Some fans stall at low PWM values. If a channel's curve produces a
duty below the fan's physical minimum, it stops spinning entirely.
Setting `min_duty` on a channel prevents this:

```toml
[[channels]]
pwm = "pwm1"
source = "cpu"
min_duty = 15.0  # never drop below 15%
```

The floor applies even when the curve would otherwise turn the fan
off. Overrides bypass the floor since they're explicitly set by the
user. The value is a percentage (0-100), and channels without
`min_duty` can still stop when cool.

### Stall detection

If you don't know your fan's stall point up front, stall detection
catches it at runtime by reading the tachometer. When a channel is
writing duty > 0 but the RPM sensor reads 0 for several consecutive
cycles, the daemon bumps duty to `min_duty` (or 15% if no floor is
set) to restart the fan.

```toml
[[channels]]
pwm = "pwm1"
source = "cpu"
stall_detect = true      # enable RPM-based stall detection
stall_threshold = 3      # consecutive zero-RPM reads before bump (default: 3)
```

Not all fans have tachometers, so this is opt-in per channel. The
RPM sensor is mapped by index (pwm1 reads fan1_input from the
platform hwmon). `powercurve status` shows current RPM and a
`[STALLED]` tag on affected channels.

### Passthrough mode

Some fans are better left under BIOS or firmware control. Setting
`passthrough = true` on a channel tells the daemon to skip it entirely,
no duty writes, no RPM reads, no curve evaluation. The channel keeps
whatever control mode the system firmware set.

```toml
[[channels]]
pwm = "pwm4"
source = "all"
passthrough = true
```

Passthrough channels show up in `powercurve status` as `[passthrough]`
and are excluded from curves, overrides, and stall detection. Useful
for chipset fans or channels with no connected hardware.

## Monitoring and status

Check the current state of the daemon:

```
powercurve status
```

This shows the active profile, CPU/GPU temperatures, per-channel PWM
duties, and the active fan curve for each channel. Doesn't require root.

Check the version:

```
powercurve version
```

### Temporary fan overrides

For testing and tuning, you can temporarily override a fan channel's
duty cycle without changing the config:

```
powercurve fan pwm3 50
```

This sets pwm3 to 50% immediately. The override persists until you
clear it or switch profiles:

```
powercurve fan pwm3 clear
```

Overrides show up in `powercurve status` with an `[override]` tag
next to the affected channel. Switching profiles clears all overrides
automatically.

### Finding the spin-up floor

If you're not sure what `min_duty` to set for a channel, `fan-test`
finds it for you. It ramps duty upward from 5% in 5% increments,
reading RPM at each step, and reports the lowest duty where the fan
actually spins:

```
powercurve fan-test pwm1
```

For finer resolution or a different starting point:

```
powercurve fan-test pwm3 --step 3 --start 10
```

The test uses the daemon's override mechanism so other fans keep
running normally. The override is cleared when the test finishes
or if you hit Ctrl-C. Use the reported value as your `min_duty`
in fan.toml.

For desktop notifications on profile switches and thermal events, run
the monitor as a user service:

```
systemctl --user enable --now powercurve-monitor
```

The monitor listens for D-Bus signals from the daemon and sends
notifications through `org.freedesktop.Notifications`. It runs as a
separate process so it doesn't need root.

## Config validation

Validate your config without restarting the daemon:

```
powercurve config
```

This checks curve monotonicity, duty ranges, critical temp bounds,
hysteresis/cooldown values, profile names, per-channel profile curves,
and whether the referenced hwmon devices exist on the current machine.
Errors prevent the daemon from loading the config, warnings are
informational.

## Hot reload

Edit the config and reload without restarting:

```
sudo systemctl reload powercurve
```

The daemon validates the new config before applying it. If validation
fails, it keeps the running config and logs the errors. The active
profile's curves are re-applied after reload so channels don't fall
back to the shared curve.

## Building

Requires a stable Rust toolchain:

```
make
sudo make install
```

The daemon runs as a systemd service:

```
sudo systemctl enable --now com.vintagetechie.PowerCurve
```

## Rollback

See [ROLLBACK.md](ROLLBACK.md) for instructions on switching back to
power-profiles-daemon or downgrading to a previous version.

## License

GPL-3.0-only

Based on [system76-power](https://github.com/pop-os/system76-power)
by System76, also licensed under GPL-3.0-only.
