// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

pub(crate) mod acpi_platform;
pub mod args;
pub mod client;
pub mod config_check;
pub(crate) mod cpufreq;
pub mod daemon;
pub(crate) mod errors;
pub mod fan;
pub mod fan_detect;
pub(crate) mod fan_test;
pub mod graphics;
pub(crate) mod kernel_parameters;
pub mod logging;
pub mod monitor;
pub mod nvml;
pub(crate) mod pci;
pub(crate) mod radeon;
pub(crate) mod state;
pub(crate) mod util;
pub mod watcher;

pub(crate) static DBUS_NAME: &str = "com.vintagetechie.PowerCurve";
pub(crate) static DBUS_PATH: &str = "/com/vintagetechie/PowerCurve";

#[derive(Copy, Clone, Debug)]
pub(crate) enum Profile {
    Quiet,
    Balanced,
    Performance,
}
