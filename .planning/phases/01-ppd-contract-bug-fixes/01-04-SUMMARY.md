---
phase: 01-ppd-contract-bug-fixes
plan: "04"
subsystem: state
tags: [correctness, durability, atomic-write, crash-resilience]
dependency_graph:
  requires: []
  provides: [COR-13]
  affects: [src/state.rs]
tech_stack:
  added: []
  patterns: [temp-file-plus-rename, sync_all-before-rename, OpenOptionsExt-mode]
key_files:
  modified: [src/state.rs]
decisions:
  - "Used std::fs::rename (not tokio::fs) — save_profile is off the 1 Hz tick path and already synchronous; keeps diff minimal"
  - "Per-PID temp suffix (profile.tmp.{pid}) — sufficient for single-instance daemon, no external deps"
  - "Unknown content in load_profile left untouched — preserves user intent across upgrades that rename profiles"
metrics:
  duration: "~5 minutes"
  completed: "2026-05-17T22:09:35Z"
  tasks_completed: 1
  tasks_total: 2
  files_changed: 1
---

# Phase 01 Plan 04: Atomic state file save Summary

`save_profile` writes to a temp file in `/var/lib/powercurve/`, fsyncs, and atomically renames onto the state file. A kill -9 mid-write now leaves the previous valid state intact.

## Tasks

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Atomic save via temp file plus rename | e18c0db | src/state.rs |
| 2 | Manual crash-resilience verification | CHECKPOINT | — |

## What Changed

`src/state.rs` — `save_profile` now:
1. Builds a temp path in `STATE_DIR`: `profile.tmp.{pid}`
2. Opens with `OpenOptions` + `mode(0o644)` — explicit creation permissions
3. Writes, flushes, `sync_all()` — durable before the directory-entry swap
4. Drops the file handle before rename
5. `fs::rename(temp, STATE_FILE)` — atomic on POSIX same-filesystem
6. Best-effort `fs::remove_file(temp)` on any intermediate error, with `// Why dropped:` comment on each discarded `Result`

`load_profile` is unchanged in behavior. The doc comment now explicitly states that unknown content is retained on disk so a future upgrade can detect and migrate the old name rather than silently losing user intent.

## Deviations from Plan

None — plan executed exactly as written.

## Known Stubs

None.

## Threat Flags

None. This change narrows the attack surface: a world-writable temp file in `/var/lib/powercurve/` is not introduced because the directory is root-owned and the daemon runs as root.

## Checkpoint Status

Task 2 is a `checkpoint:human-verify` gate. Manual crash-resilience and drop-in contract verification required before this plan is marked complete.

## Self-Check: PASSED

- `src/state.rs` exists and contains `fs::rename`, `sync_all`, `OpenOptions`, `profile.tmp.`, `0o644`
- Commit e18c0db exists
- `cargo build -p powercurve` clean
- `cargo clippy --workspace -- -D warnings` clean
- `fs::write(STATE_FILE` — zero matches (non-atomic write removed)
- `load_profile` — no `fs::write`, `fs::remove_file`, or `OpenOptions` against `STATE_FILE`
