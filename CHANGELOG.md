# Changelog

## Unreleased

A pass over the fan-control failure paths, all biased toward keeping fans
spinning when something goes wrong rather than letting them coast.

A dead temperature source no longer pins fans low. If a sensor stops
reporting, a channel held its last duty — which could be the floor, or even
zero — indefinitely, and a channel with a min_duty floor dropped straight to
that floor with no warning. Channels now hold their last duty but never below
the spin-up fallback or the configured floor, log the missing source once, and
escalate to full speed after 30 seconds without a reading, since critical-temp
detection is blind while the sensor is gone.

NVML being unavailable no longer cripples the whole system. When NVIDIA
hardware was present but its temperature could not be read, the daemon forged a
critical reading, which drove *every* fan to max, logged once a second, and —
with thermal fallback on — stepped the profile down to Quiet and never
recovered. Only the GPU-fed channels are now forced to full speed; CPU channels
keep following their curves and the profile is left alone.

Fan curves with decreasing duty are now rejected at validation, and curve
interpolation can no longer wrap around if such a curve reaches the evaluator —
previously a duty that fell as temperature rose produced a garbage value
(release) or panicked the daemon (debug).

fan-test clears its override on every exit path. An error mid-test or a SIGTERM
used to leave the channel pinned at a low test duty in the daemon until the
next profile change; only Ctrl-C cleaned up.

Failed hwmon writes and hwmon discovery loss are no longer silent. A PWM write
the EC rejects, and a mid-run discovery failure that suspends fan control, are
now logged once per episode (with a matching recovery line) instead of leaving
the daemon quietly believing it is in control. A persistently stalled fan also
logs one warning per stall episode instead of one every second.

## 0.4.0 (2026-06-04)

Temperature reads, fan overrides, and startup config handling all get more
correct, plus a few internal cleanups.

Fan channels now track the hottest sensor on each hwmon chip instead of just
the first. The daemon was only reading temp1, which on multi-sensor packages
(per-core, junction, hotspot) could sit well below the actual peak. It now
takes the max across every tempN_input, so curves respond to the temperature
that matters.

Temporary fan overrides read back exactly. Setting a channel to 50% and
querying it returned 49%, because the percent was round-tripped through a raw
PWM byte and back. Overrides are now stored as a percent and converted to a
byte only when applied.

The daemon validates the fan config at startup, not just on reload. A config
with errors — a non-monotonic curve, a duty that goes quiet at the critical
temperature — was rejected on SIGHUP but silently applied at boot. Both paths
now agree: an invalid config is refused and the daemon runs profile-only until
it is fixed, with the reason logged.

hwmon discovery is cached instead of re-enumerated every second. The fan loop
re-scans sysfs every 30 seconds while healthy, and immediately when a scan
fails, so a late-appearing sensor at boot still recovers within a tick without
the daemon walking /sys/class/hwmon at 1 Hz forever.

Profile-application logging that bypassed the log framework (raw stderr writes)
now goes through the normal logger and respects the daemon's log level. The
Arch install validates the generated fan.toml the same way the Debian package
already did, so install-time config problems are visible.

## 0.3.1 (2026-05-18)

Four bug fixes against the power-profiles-daemon drop-in surface. GNOME and KDE
inhibitor UIs were seeing empty data where they should have been seeing live
holds, and a malformed state-file write could leave the daemon reading a
half-truncated profile name on boot.

When a HoldProfile cookie gets released, the ProfileReleased signal now
actually fires. The future returned by the signal emitter was being dropped
unawaited, which made the signal a no-op on the wire. Inhibitor UIs that
watch for release events finally get told when a hold drops.

ActiveProfileHolds returns the live list of held profiles with the Profile,
ApplicationId, and Reason keys the upstream interface defines. The old
implementation returned an empty array. Anything querying the property for
state restoration or display saw nothing.

set_active_profile rejects unknown profile names with InvalidArgs and a WARN
log instead of silently doing nothing. Typos and stale clients no longer
leave the user wondering why their profile change vanished.

State-file writes go through temp file plus sync_all plus atomic rename. A
power loss or crash mid-write can no longer leave a half-truncated profile
name that the daemon will refuse to parse on next boot.

## 0.3.0 (2026-05-06)

When a temperature source disappears mid-run, the daemon now holds the last
known duty instead of dropping the channel. Lost a sensor read for one cycle?
The fan keeps doing what it was doing. Lose it long enough that the
critical-temp safety net trips? You still get max duty. The old behavior of
sailing to zero on a missing read was a bug waiting to happen.

Config validation now rejects `duty = 0` at or above the critical temperature.
A curve that says "go quiet at 90C" is a misconfiguration, not a preference.
The daemon refuses to load it. Existing configs without that pattern keep
working untouched.

Retry exhaustion and signal-handler failures now surface as real errors
instead of being swallowed. If something gives up or a handler fails, you
see it in the journal.

The workspace moved to Rust edition 2024, the lockfile refreshed against
current advisories, and toml and thiserror picked up their respective major
version bumps. All internal, nothing for users to do.

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
