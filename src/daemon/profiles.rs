// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

use super::pci_runtime_pm_support;
use crate::{
    errors::{ModelError, PciDeviceError, ProfileError, ScsiHostError},
    kernel_parameters::{DeviceList, Dirty},
    radeon::RadeonDevice,
    Profile,
};
use intel_pstate::{PState, PStateError, PStateValues};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    process::Command,
};
use sysfs_class::{PciDevice, RuntimePM, RuntimePowerManagement, ScsiHost, SysClass};

/// Instead of returning on the first error, we want to collect all errors that occur while
/// setting a profile. Even if one parameter fails to set, we'll still be able to set other
/// parameters successfully.
macro_rules! catch {
    ($errors:ident, $result:expr) => {
        match $result {
            Ok(_) => (),
            Err(why) => $errors.push(why.into()),
        }
    };
}

/// Sets parameters for the balanced profile.
pub fn balanced(errors: &mut Vec<ProfileError>) {
    if crate::acpi_platform::supported() {
        crate::acpi_platform::balanced();
    }

    // How often the OS syncs data to disk. 15s balances power saving against
    // data loss risk from sudden power loss.
    Dirty::default().set_max_lost_work(15);

    RadeonDevice::get_devices().for_each(|dev| dev.set_profiles("auto", "performance", "auto"));

    catch!(errors, scsi_host_link_time_pm_policy(&["med_power_with_dipm", "medium_power"]));

    if pci_runtime_pm_support() {
        catch!(errors, pci_device_runtime_pm(RuntimePowerManagement::On));
    }

    crate::cpufreq::set(Profile::Balanced, 100);

    catch!(
        errors,
        pstate_values(
            PStateValues::default()
                .hwp_dynamic_boost(true)
                .min_perf_pct(0)
                .max_perf_pct(100)
                .no_turbo(false)
        )
    );

    if let Some(model_profiles) = ModelProfiles::new() {
        catch!(errors, model_profiles.balanced.set());
    }
}

/// Sets parameters for the performance profile.
pub fn performance(errors: &mut Vec<ProfileError>) {
    if crate::acpi_platform::supported() {
        crate::acpi_platform::performance();
    }

    Dirty::default().set_max_lost_work(15);
    RadeonDevice::get_devices().for_each(|dev| dev.set_profiles("high", "performance", "auto"));
    catch!(errors, scsi_host_link_time_pm_policy(&["med_power_with_dipm", "max_performance"]));
    crate::cpufreq::set(Profile::Performance, 100);
    catch!(
        errors,
        pstate_values(
            PStateValues::default()
                .hwp_dynamic_boost(true)
                .min_perf_pct(0)
                .max_perf_pct(100)
                .no_turbo(false)
        )
    );

    if pci_runtime_pm_support() {
        catch!(errors, pci_device_runtime_pm(RuntimePowerManagement::Off));
    }

    if let Some(model_profiles) = ModelProfiles::new() {
        catch!(errors, model_profiles.performance.set());
    }
}

/// Sets parameters for the quiet profile. Reduces CPU clocks and enables
/// aggressive power management for a quieter, cooler desktop.
pub fn quiet(errors: &mut Vec<ProfileError>) {
    if crate::acpi_platform::supported() {
        crate::acpi_platform::battery();
    }

    Dirty::default().set_max_lost_work(15);
    RadeonDevice::get_devices().for_each(|dev| dev.set_profiles("low", "battery", "low"));
    catch!(errors, scsi_host_link_time_pm_policy(&["min_power", "min_power"]));
    crate::cpufreq::set(Profile::Quiet, 50);

    catch!(
        errors,
        pstate_values(PStateValues::default().min_perf_pct(0).max_perf_pct(50).no_turbo(true))
    );

    if pci_runtime_pm_support() {
        catch!(errors, pci_device_runtime_pm(RuntimePowerManagement::On));
    }

    if let Some(model_profiles) = ModelProfiles::new() {
        catch!(errors, model_profiles.quiet.set());
    }
}

/// Controls the Intel [`PState`] values.
fn pstate_values(values: PStateValues) -> Result<(), PStateError> {
    if let Ok(pstate) = PState::new() {
        pstate.set_values(values)?;
    }

    Ok(())
}

/// Iterates on all available PCI devices, disabling or enabling runtime power management.
fn pci_device_runtime_pm(pm: RuntimePowerManagement) -> Result<(), PciDeviceError> {
    for device in PciDevice::iter() {
        match device {
            Ok(device) => device
                .set_runtime_pm(pm)
                .map_err(|why| PciDeviceError::SetRuntimePm(device.id().to_owned(), why))?,
            Err(why) => {
                log::warn!("failed to iterate PCI device: {}", why);
            }
        }
    }

    Ok(())
}

/// Iterates on all available SCSI/SATA hosts, setting the first link time power management policy
/// that succeeds.
fn scsi_host_link_time_pm_policy(policies: &'static [&'static str]) -> Result<(), ScsiHostError> {
    for device in ScsiHost::iter() {
        match device {
            Ok(device) => {
                device.set_link_power_management_policy(policies).map_err(|why| {
                    ScsiHostError::LinkTimePolicy(policies[0], device.id().to_owned(), why)
                })?;
            }
            Err(why) => {
                log::warn!("failed to iterate SCSI Host device: {}", why);
            }
        }
    }

    Ok(())
}

/// Per-model power limit and thermal tuning. Keeps the framework for adding
/// desktop-specific profiles later.
pub struct ModelProfile {
    pl1:        Option<u8>,
    pl2:        Option<u8>,
    tcc_offset: Option<u8>,
}

impl ModelProfile {
    pub fn set(&self) -> Result<(), ModelError> {
        // Thermald sets pl1 and pl2 on its own, conflicting with us
        let _status = Command::new("systemctl")
            .arg("stop")
            .arg("thermald.service")
            .status()
            .map_err(ModelError::Thermald)?;

        if let Some(pl1) = self.pl1 {
            fs::write(
                "/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw",
                format!("{}", u64::from(pl1) * 1_000_000),
            )
            .map_err(ModelError::Pl1)?;
        }

        if let Some(pl2) = self.pl2 {
            fs::write(
                "/sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw",
                format!("{}", u64::from(pl2) * 1_000_000),
            )
            .map_err(ModelError::Pl2)?;
        }

        if let Some(tcc_offset) = self.tcc_offset {
            let path = Path::new("/dev/cpu/0/msr");
            if !path.is_file() {
                let status =
                    Command::new("modprobe").arg("msr").status().map_err(ModelError::ModprobeIo)?;
                if !status.success() {
                    return Err(ModelError::ModprobeExitStatus(status));
                }
            }

            let mut file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(ModelError::MsrOpen)?;
            file.seek(SeekFrom::Start(0x1A2)).map_err(ModelError::MsrSeek)?;
            let mut data = [0; 8];
            file.read_exact(&mut data).map_err(ModelError::MsrRead)?;
            data[3] = tcc_offset;
            file.write_all(&data).map_err(ModelError::MsrWrite)?;
        }

        Ok(())
    }
}

pub struct ModelProfiles {
    pub balanced:    ModelProfile,
    pub performance: ModelProfile,
    pub quiet:       ModelProfile,
}

impl ModelProfiles {
    /// Returns model-specific power profiles if the running hardware has a
    /// known configuration. Currently empty, ready for desktop entries.
    pub fn new() -> Option<Self> {
        let _model_line =
            fs::read_to_string("/sys/class/dmi/id/product_version").unwrap_or_default();
        // Add desktop model entries here as needed.
        None
    }
}
