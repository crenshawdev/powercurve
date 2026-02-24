# vintagetechie-power

Desktop power management daemon with per-channel fan control. Forked from
[system76-power](https://github.com/pop-os/system76-power) and stripped down
for desktop use only, no laptop, battery, or graphics switching code.

## Installation

Available from the [VintageTechie Arch repo](https://codeberg.org/VintageTechie/vintagetechie-arch-repo):

```
sudo pacman -S vintagetechie-power-git
```

The package provides `power-profiles-daemon` so desktop environments that
talk to the `org.freedesktop.UPower.PowerProfiles` D-Bus interface
(GNOME, KDE) will pick up profiles automatically.

## Power profiles

Three profiles, switchable via D-Bus or the CLI:

- **Quiet** maps to `power-saver`. Conservative CPU governor, longer dirty
  data writeback intervals.
- **Balanced** is the default. Moderate CPU scaling, NMI watchdog off.
- **Performance** unlocks full CPU frequency and turbo boost.

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

Desktop environments that use `powerprofilesctl` will work without any
extra configuration.

## Fan control

Fan curves and channel mapping are configured in
`/etc/vintagetechie-power/fan.toml`. Each channel maps a PWM output to a
temperature source (`cpu`, `gpu`, or `all`):

```toml
critical_cpu_temp = 85
critical_gpu_temp = 80

[[curve]]
temp = 35.0
duty = 0

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

Temperatures are in Celsius, duty is a percentage (0-100). If any sensor
crosses its critical threshold, all fans go to max. Without a config file
the daemon falls back to built-in curves based on DMI product version
(Thelio models).

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
