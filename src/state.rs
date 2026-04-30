// Copyright 2024 VintageTechie
//
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::path::Path;

const STATE_DIR: &str = "/var/lib/powercurve";
const STATE_FILE: &str = "/var/lib/powercurve/profile";

const VALID_PROFILES: &[&str] = &["Quiet", "Balanced", "Performance"];

/// Write the active profile name to disk so it survives restarts.
pub fn save_profile(name: &str) {
    let dir = Path::new(STATE_DIR);
    if !dir.exists()
        && let Err(why) = fs::create_dir_all(dir)
    {
        log::warn!("failed to create state directory {STATE_DIR}: {why}");
        return;
    }

    if let Err(why) = fs::write(STATE_FILE, name) {
        log::warn!("failed to save profile state to {STATE_FILE}: {why}");
    }
}

/// Read the last saved profile from disk. Returns None if the file is
/// missing, empty, or contains something unexpected.
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
