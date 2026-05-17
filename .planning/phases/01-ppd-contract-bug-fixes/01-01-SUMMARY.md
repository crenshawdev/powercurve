---
phase: 01-ppd-contract-bug-fixes
plan: "01"
subsystem: dbus
tags: [zbus, upower, power-profiles-daemon, signal]

requires: []
provides:
  - "UPowerPowerProfiles::release_profile now awaits the profile_released signal future"
affects: [01-02, 01-03, 01-04, ppd-contract-verification]

tech-stack:
  added: []
  patterns: ["let _res = ...await for non-fatal async signal sends"]

key-files:
  created: []
  modified: [src/daemon.rs]

key-decisions:
  - "Added .await to Self::profile_released call; kept let _res = pattern so signal failure doesn't abort the release path after hold has already been removed"

patterns-established:
  - "Non-fatal signal send: let _res = Self::signal_fn(&context, args).await — matches emit_active_profile_changed convention"

requirements-completed: [PPD-01]

duration: 5min
completed: 2026-05-17
---

# Phase 01 Plan 01: PPD-01 ProfileReleased Signal Fix Summary

**`release_profile` now awaits the `profile_released` signal future, so inhibitor consumers (GNOME, KDE) receive `ProfileReleased` when a hold drops**

## Performance

- **Duration:** ~5 min
- **Started:** 2026-05-17T22:05:00Z
- **Completed:** 2026-05-17T22:09:15Z
- **Tasks:** 1 executed, 1 checkpoint reached
- **Files modified:** 1

## Accomplishments

- Fixed PPD-01: added `.await` to `Self::profile_released(&context, cookie)` in `UPowerPowerProfiles::release_profile`
- Build and clippy both clean with no new warnings
- Manual D-Bus verification checkpoint queued for live confirmation

## Task Commits

1. **Task 1: Await the profile_released signal future** - `cb3c6d2` (fix)

**Plan metadata:** (docs commit follows)

## Files Created/Modified

- `src/daemon.rs` - added `.await` to `Self::profile_released` call at line 427

## Decisions Made

Kept the `let _res = ...` discard pattern. The hold has already been removed from `held_profiles` before this signal send; a transient D-Bus failure here must not abort the release path or surface an error to the caller.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

Manual D-Bus verification required before merging. The checkpoint instructions are in the plan at `.planning/phases/01-ppd-contract-bug-fixes/01-01-PLAN.md` Task 2. Steps:

1. `sudo make install` from `/code/powercurve`, then `sudo systemctl restart powercurve`
2. Run `dbus-monitor` in one terminal watching `org.freedesktop.UPower.PowerProfiles ProfileReleased`
3. HoldProfile + ReleaseProfile via busctl; confirm signal fires
4. `powerprofilectl list/get/set/launch` from active session (no auth prompt expected)

## Next Phase Readiness

- PPD-01 source fix is committed and clean
- Awaiting manual D-Bus verification from active desktop session before this fix can be declared fully verified
- Plans 01-02, 01-03, 01-04 are independent P0 fixes in the same phase and can proceed in parallel

---
*Phase: 01-ppd-contract-bug-fixes*
*Completed: 2026-05-17*
