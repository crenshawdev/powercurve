# Changelog

## 0.2.1 (2026-04-15)

Switched the systemd unit to Type=notify with a watchdog. systemd actually
knows when the daemon is alive instead of just assuming it. The fan loop pings
the watchdog after each step. A hung step trips it instead of sailing past.

Radeon detection now reads the PCI vendor ID before probing DPM paths. Machines
with Intel or Nvidia graphics no longer generate spurious card probes. Also
walks /sys/class/drm for real card entries instead of guessing 0 through 9.

Postinst pipes the config check output to the journal after generating a
default fan config. Install-time problems are visible without hunting.

## 0.2.0 (2026-03-27)

Process watcher. `powercurve watch` polls /proc and switches profiles based
on rules in ~/.config/powercurve/watcher.toml. Runs as a user service or
from the terminal. No config, no behavior.

## 0.1.0-rc.1 (2026-02-27)

Channels can now be set to passthrough mode, leaving them under BIOS or firmware
control instead of writing duties. Useful for chipset fans or channels with no
connected hardware.

Shipped the first batch of example configs at three complexity levels (simple,
desktop, full-featured) so new users have something to start from besides the
raw reference docs. Also added a proper man page covering all commands, config
options, and the D-Bus interface.

## 0.1.0-beta.6 (2026-02-27)

The daemon now emits a StallEvent D-Bus signal when a fan stall is detected, and
the monitor service picks it up for desktop notifications. Previously stalls were
only visible in the logs or via `powercurve status`.

Added `fan-test`, a command that ramps a fan channel from low to high duty in
configurable steps, reading RPM at each level to find the minimum duty where the
fan actually spins. Makes finding the right `min_duty` value straightforward
instead of guessing.

## 0.1.0-beta.5.2 (2026-02-27)

Big feature batch. `powercurve version` now exists for the obvious reason.
`powercurve status` shows active fan curves per channel and flags overrides.
Temporary fan overrides let you set a channel to a fixed duty for testing without
touching the config, and they clear automatically on profile switch.

Per-channel minimum duty floors prevent fans from stalling at low PWM values. If
the curve produces a duty below the floor, the fan holds at `min_duty` instead
of stopping. Stall detection via RPM tachometer catches it at runtime too, bumping
duty back up when a fan stops spinning unexpectedly.

Fixed the Makefile uninstall path and cleaned up the Arch post_remove script.

## 0.1.0-beta.5.1 (2026-02-27)

When the daemon shuts down, it now resets all PWM channels to firmware control
(`pwm_enable = 2`) so fans don't get stuck at whatever duty was last written.
The systemd service file handles this in ExecStopPost as a safety net.

## 0.1.0-beta.5 (2026-02-26)

Fixed thermal settings being ignored after a hot reload. The daemon was caching
critical temps and hysteresis from the initial config load and not picking up
changes on SIGHUP. Now reads thermal settings each loop iteration from the
active config.

## 0.1.0-beta.4 (2026-02-26)

Added a prominent desktop-only note to the README since people kept trying it on
laptops where fan control can't work.

Fixed the daemon not staying enabled across package upgrades on Arch. Also fixed
a shutdown hang where the daemon would block waiting for D-Bus cleanup that was
never going to happen.

## 0.1.0-beta.3 (2026-02-26)

Per-channel per-profile curves, completing the four-layer curve system. You can
now give each individual fan its own curve on each profile.

`fan-detect` got smarter about critical temperatures, reading the CPU's thermal
throttle point from sysfs and setting the critical threshold 15C below it instead
of using a hardcoded value.

Stripped out a bunch of dead legacy code inherited from system76-power and added
the first real test coverage for config validation.

Fixed thermal fallback resetting itself on profile change, and fixed SIGHUP
killing the monitor service. Also fixed profile curves not being reapplied after
a config reload.

## 0.1.0-beta.2 (2026-02-26)

The big feature sprint. Fan hysteresis prevents rapid speed cycling when temps
hover around curve points. Hot config reload via SIGHUP means you can tweak
curves without restarting the daemon. Per-profile global curves let you run
different fan behavior on each power profile.

Added `powercurve status` for checking the daemon state from the CLI, and
`powercurve monitor` for desktop notifications on profile switches and thermal
events. The monitor runs as a systemd user service.

Config validation catches bad curves and invalid values before the daemon tries
to apply them. D-Bus connection retries with exponential backoff replaced the
old "fail once and die" approach.

## 0.1.0-beta.1 (2026-02-26)

Renamed from vintagetechie-power to powercurve. Added Debian packaging (.deb)
alongside the existing Arch PKGBUILD.

## 0.1.0-alpha.1 (2026-02-26)

First public release. Forked from system76-power with the system76-specific
hardware assumptions stripped out.

Replaced the nvidia-smi subprocess call with direct NVML loading via dlopen,
removing the hard dependency on the NVIDIA CLI tools. Fan control works through
any hwmon device with PWM outputs, not just the hardcoded chips from the original
project.

Three power profiles (Quiet, Balanced, Performance) with CPU governor, turbo
boost, PCI runtime PM, and SCSI link policy management. Compatible with the
freedesktop PowerProfiles D-Bus interface so desktop environments pick up
profiles automatically.
