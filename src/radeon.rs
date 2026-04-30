// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

use crate::kernel_parameters::{
    DeviceList, KernelParameter, RadeonDpmForcePerformance, RadeonDpmState, RadeonPowerMethod,
    RadeonPowerProfile,
};

/// AMD PCI vendor ID.
const AMD_VENDOR_ID: u16 = 0x1002;

/// Check whether a DRM device belongs to AMD by reading its PCI vendor ID.
fn is_amd_device(device_path: &str) -> bool {
    let vendor_path = format!("{device_path}/vendor");
    std::fs::read_to_string(&vendor_path)
        .ok()
        .and_then(|v| {
            let trimmed = v.trim();
            let hex = trimmed.trim_start_matches("0x").trim_start_matches("0X");
            u16::from_str_radix(hex, 16).ok()
        })
        .map(|id| id == AMD_VENDOR_ID)
        .unwrap_or(false)
}

/// Scan `/sys/class/drm` for `cardN` entries, yielding each card index.
fn drm_card_indices() -> Vec<u8> {
    let entries = match std::fs::read_dir("/sys/class/drm") {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut cards: Vec<u8> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name();
            let name_str = name.to_str()?;
            let stripped = name_str.strip_prefix("card")?;
            if stripped.contains('-') {
                return None;
            }
            stripped.parse::<u8>().ok()
        })
        .collect();
    cards.sort_unstable();
    cards
}

pub struct RadeonDevice {
    card: u8,
    pub dpm_state: RadeonDpmState,
    pub dpm_force_performance: RadeonDpmForcePerformance,
    pub power_method: RadeonPowerMethod,
    pub power_profile: RadeonPowerProfile,
}

impl RadeonDevice {
    #[must_use]
    pub fn new(card: u8) -> Option<Self> {
        let path = format!("/sys/class/drm/card{card}/device");

        if !is_amd_device(&path) {
            return None;
        }

        let device = Self {
            card,
            dpm_state: RadeonDpmState::new(&path),
            dpm_force_performance: RadeonDpmForcePerformance::new(&path),
            power_method: RadeonPowerMethod::new(&path),
            power_profile: RadeonPowerProfile::new(&path),
        };

        let exists = device.dpm_state.get_path().exists()
            && device.dpm_force_performance.get_path().exists()
            && device.power_method.get_path().exists()
            && device.power_profile.get_path().exists();

        if exists { Some(device) } else { None }
    }

    pub fn set_profiles(&self, power_profile: &str, dpm_state: &str, dpm_perf: &str) {
        log::debug!(
            "Setting radeon{} to power profile {}; DPM state {}; DPM perf {}",
            self.card,
            power_profile,
            dpm_state,
            dpm_perf
        );
        self.dpm_state.set(dpm_state.as_bytes());
        self.dpm_force_performance.set(dpm_perf.as_bytes());
        self.power_method.set(b"profile");
        self.power_profile.set(power_profile.as_bytes());
    }
}

impl DeviceList<Self> for RadeonDevice {
    fn get_devices() -> Box<dyn Iterator<Item = Self>> {
        Box::new(drm_card_indices().into_iter().filter_map(Self::new))
    }
}
