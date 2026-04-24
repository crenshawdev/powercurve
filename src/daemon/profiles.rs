// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

use super::pci_runtime_pm_support;
use crate::{
    Profile,
    errors::{PciDeviceError, ProfileError},
    kernel_parameters::{DeviceList, Dirty},
    radeon::RadeonDevice,
};
use intel_pstate::{PState, PStateError, PStateValues};
use sysfs_class::{PciDevice, RuntimePM, RuntimePowerManagement, ScsiHost, SysClass};

/// Instead of returning on the first error, we want to collect all errors that occur while
/// setting a profile. Even if one parameter fails to set, we'll still be able to set other
/// parameters successfully.
macro_rules! catch {
    ($errors:ident, $result:expr_2021) => {
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

    scsi_host_link_time_pm_policy(&["med_power_with_dipm", "medium_power"]);

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
}

/// Sets parameters for the performance profile.
pub fn performance(errors: &mut Vec<ProfileError>) {
    if crate::acpi_platform::supported() {
        crate::acpi_platform::performance();
    }

    Dirty::default().set_max_lost_work(15);
    RadeonDevice::get_devices().for_each(|dev| dev.set_profiles("high", "performance", "auto"));
    scsi_host_link_time_pm_policy(&["med_power_with_dipm", "max_performance"]);
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
}

/// Sets parameters for the quiet profile. Reduces CPU clocks and enables
/// aggressive power management for a quieter, cooler desktop.
pub fn quiet(errors: &mut Vec<ProfileError>) {
    if crate::acpi_platform::supported() {
        crate::acpi_platform::quiet();
    }

    Dirty::default().set_max_lost_work(15);
    RadeonDevice::get_devices().for_each(|dev| dev.set_profiles("low", "battery", "low"));
    scsi_host_link_time_pm_policy(&["min_power", "min_power"]);
    crate::cpufreq::set(Profile::Quiet, 50);

    catch!(
        errors,
        pstate_values(PStateValues::default().min_perf_pct(0).max_perf_pct(50).no_turbo(true))
    );

    if pci_runtime_pm_support() {
        catch!(errors, pci_device_runtime_pm(RuntimePowerManagement::On));
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
/// that succeeds. Hosts that don't support the requested policy are logged and skipped.
fn scsi_host_link_time_pm_policy(policies: &'static [&'static str]) {
    for device in ScsiHost::iter() {
        match device {
            Ok(device) => {
                if let Err(why) = device.set_link_power_management_policy(policies) {
                    log::debug!("scsi host {}: {}", device.id(), why);
                }
            }
            Err(why) => {
                log::warn!("failed to iterate SCSI Host device: {}", why);
            }
        }
    }
}
