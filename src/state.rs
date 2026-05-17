// Copyright 2024 VintageTechie
//
// SPDX-License-Identifier: GPL-3.0-only

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const STATE_DIR: &str = "/var/lib/powercurve";
const STATE_FILE: &str = "/var/lib/powercurve/profile";

const VALID_PROFILES: &[&str] = &["Quiet", "Balanced", "Performance"];

/// Write the active profile name to disk so it survives restarts.
///
/// Uses a temp-file-plus-rename pattern so a kill -9 mid-write leaves
/// the previous valid state file intact rather than an empty or partial one.
pub fn save_profile(name: &str) {
    let dir = Path::new(STATE_DIR);
    if !dir.exists()
        && let Err(why) = fs::create_dir_all(dir)
    {
        log::warn!("failed to create state directory {STATE_DIR}: {why}");
        return;
    }

    // Temp file must live in the same directory as the target so the
    // rename(2) stays on the same filesystem — cross-fs rename is not atomic.
    let temp_path = Path::new(STATE_DIR).join(format!("profile.tmp.{}", std::process::id()));

    let mut file = match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(&temp_path)
    {
        Ok(f) => f,
        Err(why) => {
            log::warn!("failed to open state temp file {}: {why}", temp_path.display());
            return;
        }
    };

    if let Err(why) = file.write_all(name.as_bytes()) {
        log::warn!("failed to write state temp file {}: {why}", temp_path.display());
        // Why dropped: best-effort cleanup; the next save will overwrite.
        let _ = fs::remove_file(&temp_path);
        return;
    }

    if let Err(why) = file.flush() {
        log::warn!("failed to flush state temp file {}: {why}", temp_path.display());
        // Why dropped: best-effort cleanup; the next save will overwrite.
        let _ = fs::remove_file(&temp_path);
        return;
    }

    if let Err(why) = file.sync_all() {
        log::warn!("failed to sync state temp file {}: {why}", temp_path.display());
        // Why dropped: best-effort cleanup; the next save will overwrite.
        let _ = fs::remove_file(&temp_path);
        return;
    }

    // Drop the file handle before rename so the fd is closed and all
    // OS-level buffers are committed prior to the directory-entry swap.
    drop(file);

    if let Err(why) = fs::rename(&temp_path, STATE_FILE) {
        log::warn!("failed to atomically rename state file to {STATE_FILE}: {why}");
        // Why dropped: best-effort cleanup; the next save will try again.
        let _ = fs::remove_file(&temp_path);
    }
}

/// Read the last saved profile from disk. Returns `None` if the file is
/// missing, empty, or contains something unexpected.
///
/// Unknown content is intentionally left on disk untouched. A future upgrade
/// that renames profiles (e.g. `Battery` to `Quiet`) can detect the old name
/// via the WARN log and migrate rather than silently losing user intent.
pub fn load_profile() -> Option<String> {
    let contents = fs::read_to_string(STATE_FILE).ok()?;
    let trimmed = contents.trim();

    if VALID_PROFILES.contains(&trimmed) {
        Some(trimmed.to_string())
    } else {
        log::warn!("ignoring unknown saved profile: {trimmed:?}");
        None
    }
}
