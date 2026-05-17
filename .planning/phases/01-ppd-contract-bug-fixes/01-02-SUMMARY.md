---
phase: 01-ppd-contract-bug-fixes
plan: "02"
subsystem: dbus
tags: [zbus, upower, power-profiles-daemon, inhibitor]

requires: []
provides:
  - "UPowerPowerProfiles::active_profile_holds returns the live held-profiles list in upstream PPD wire format"
affects: [ppd-contract-verification]

tech-stack:
  added: []
  patterns: ["Vec<HashMap<String, zvariant::Value<'static>>> via owned String clones to satisfy lifetime bounds across mutex guard"]

key-files:
  created: []
  modified: [src/daemon.rs]

key-decisions:
  - "Changed return type lifetime from '_ to 'static; clones reason and application_id strings out of the MutexGuard so the returned Vec has no borrow dependency on the guard"
  - "Used zvariant::Str::from(String) for all three values; the .into() shorthand from profiles() doesn't generalize to owned keys without type annotation noise"

requirements-completed: [PPD-02]

duration: 10min
completed: 2026-05-17
---

# Phase 01 Plan 02: PPD-02 ActiveProfileHolds Fix Summary

**`active_profile_holds` now iterates `PowerDaemon::held_profiles` and returns the live list in upstream PPD wire format with `Profile`, `ApplicationId`, and `Reason` keys**

## Performance

- **Duration:** ~10 min
- **Started:** 2026-05-17T22:06:00Z
- **Completed:** 2026-05-17T22:16:12Z
- **Tasks:** 1 executed, 1 checkpoint deferred
- **Files modified:** 1

## Accomplishments

- Fixed PPD-02: replaced the `Vec::new()` stub in `UPowerPowerProfiles::active_profile_holds` with an iterator over `PowerDaemon::held_profiles` that builds the three-key dict shape upstream PPD uses
- Changed return type from `Vec<HashMap<String, zvariant::Value<'_>>>` to `Vec<HashMap<String, zvariant::Value<'static>>>` to satisfy lifetime constraints when locking the mutex and collecting owned data
- Build and clippy clean with no new warnings

## Task Commits

1. **Task 1: Populate active_profile_holds from held_profiles** - `fafed7b` (fix)

**Plan metadata:** (docs commit follows)

## Files Created/Modified

- `src/daemon.rs` - replaced stub body in `active_profile_holds` at lines 478-501

## Decisions Made

The return type had to change from `'_` to `'static`. The original signature borrowed from the lock guard, but collecting into a `Vec` that outlives the `MutexGuard` forces all `Value` lifetimes to be owned. The fix clones `reason` and `application_id` out of the guard. `*profile` is `&'static str` (it's matched to a string literal in `hold_profile`), so `String::from(*profile)` is free.

Kept the `HashMap::from([...])` literal style matching `profiles()`. Used `zvariant::Str::from(String)` explicitly rather than `.into()` because the type inference doesn't resolve cleanly when HashMap keys are `String` instead of `&str`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Lifetime error on original implementation attempt**
- **Found during:** Task 1
- **Issue:** Initial implementation used `'_` return type which tried to return a `Vec` borrowing from the `MutexGuard<'_, PowerDaemon>` local. Rust correctly rejects this.
- **Fix:** Changed return type to `'static`, cloned `reason` and `application_id` out of the guard before collecting
- **Files modified:** `src/daemon.rs`
- **Commit:** `fafed7b`

## Checkpoints Deferred

**Task 2: Manual Properties.Get + drop-in contract verification** (`checkpoint:human-verify`, `gate="blocking"`)

Deferred per phase policy. Manual verification steps:

1. `sudo make install` from `/code/powercurve`, then `sudo systemctl restart powercurve`
2. Empty-list baseline: `dbus-send --system --print-reply --dest=org.freedesktop.UPower.PowerProfiles /org/freedesktop/UPower/PowerProfiles org.freedesktop.DBus.Properties.Get string:"org.freedesktop.UPower.PowerProfiles" string:"ActiveProfileHolds"` — expect empty array
3. Populated list: `busctl --system call org.freedesktop.UPower.PowerProfiles /org/freedesktop/UPower/PowerProfiles org.freedesktop.UPower.PowerProfiles HoldProfile sss "performance" "phase-1-test" "com.example.test"`, then re-run Properties.Get — expect one dict with `Profile=performance`, `ApplicationId=com.example.test`, `Reason=phase-1-test`
4. Release: `busctl --system call ... ReleaseProfile u <cookie>`, re-verify empty
5. Drop-in contract: `powerprofilectl list/get/set/launch` from active session, all no-prompt

## Issues Encountered

None beyond the lifetime fix above.

## Known Stubs

None introduced by this plan.

## Threat Flags

None. The change reads from existing in-process state (`held_profiles`) that is already populated by the D-Bus-exposed `HoldProfile` method. No new network surface or trust boundary introduced.

---
*Phase: 01-ppd-contract-bug-fixes*
*Completed: 2026-05-17*
