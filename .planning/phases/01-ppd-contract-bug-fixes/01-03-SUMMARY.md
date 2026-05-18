---
phase: 01-ppd-contract-bug-fixes
plan: "03"
subsystem: daemon/dbus
tags: [ppd-contract, input-validation, dbus, error-handling]
dependency_graph:
  requires: ["01-02"]
  provides: [PPD-03]
  affects: [src/daemon.rs]
tech_stack:
  added: []
  patterns: [zbus-fdo-result-property-setter, log-warn-on-rejection]
key_files:
  modified: [src/daemon.rs]
decisions:
  - "Used zbus::fdo::Error::InvalidArgs (not Failed) to distinguish input-validation failure from generic D-Bus failure"
  - "apply_held_profile discards Result with let _ because it only ever passes validated kebab-case profile names"
  - "Signal context failure returns Err(Failed) rather than silently returning"
metrics:
  duration: "~15 minutes"
  completed: "2026-05-17"
  tasks_completed: 1
  tasks_total: 2
---

# Phase 01 Plan 03: PPD-03 Input Validation Summary

`UPowerPowerProfiles::set_active_profile` now returns `zbus::fdo::Result<()>`, rejects unknown profile names with `InvalidArgs` and a `log::warn!` entry, and the `NetHadessPowerProfiles` bridge propagates the result.

## What Changed

`src/daemon.rs` - three coordinated edits:

1. `UPowerPowerProfiles::set_active_profile`: signature changed from `async fn ... -> ()` to `async fn ... -> zbus::fdo::Result<()>`. The wildcard arm `_ => return,` replaced with `log::warn!` + `Err(zbus::fdo::Error::InvalidArgs(...))`. Signal context construction failure now returns `Err(Failed(...))` instead of silently returning. The `apply_profile` result is propagated via `.map_err(zbus_error_from_display)` instead of being discarded.

2. `NetHadessPowerProfiles::set_active_profile`: signature updated to `-> zbus::fdo::Result<()>` and `.await` result propagated.

3. `UPowerPowerProfiles::apply_held_profile`: the call site now uses `let _ = self.set_active_profile(...).await` with a comment explaining the discard is intentional (the profile names there are always valid kebab-case constants).

## Tasks

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Return InvalidArgs and log on unknown profile name | 6608d82 | src/daemon.rs |
| 2 | Manual verification checkpoint | deferred | n/a |

Task 2 is a `checkpoint:human-verify` that requires a running daemon. Per checkpoint policy, it is logged here and deferred to phase end rather than blocking.

## Deviations from Plan

None - plan executed as written.

## Verification

- `grep -F "zbus::fdo::Error::InvalidArgs" src/daemon.rs` returns one match
- `grep -nF "unknown power profile" src/daemon.rs` returns two matches (hold_profile + set_active_profile)
- `grep -n "log::warn!" src/daemon.rs` includes warn in set_active_profile
- Both `set_active_profile` signatures return `zbus::fdo::Result<()>`
- Silent `_ => return,` wildcard is gone
- `cargo build -p powercurve` clean (no warnings)
- `cargo clippy --workspace -- -D warnings` clean
- 89 tests pass, no regressions

## Pending (Deferred Human Verification)

The following busctl/powerprofilectl verification steps require a live daemon and are deferred to phase-end human review:

- Valid input still applies the profile (power-saver, balanced, performance)
- Invalid input ("turbo", etc.) returns non-zero exit with D-Bus InvalidArgs error
- journalctl shows WARN line naming the bad input
- hadess bridge also rejects invalid input
- `powerprofilectl list/get/set/launch` all work no-prompt from active session

## Self-Check: PASSED

- `src/daemon.rs` modified and committed at 6608d82
- SUMMARY.md written to `.planning/phases/01-ppd-contract-bug-fixes/01-03-SUMMARY.md`
