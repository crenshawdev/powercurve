// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

pub mod acpi_platform;
pub mod args;
pub mod client;
pub mod cpufreq;
pub mod daemon;
pub mod errors;
pub mod fan;
pub mod graphics;
pub mod kernel_parameters;
pub mod logging;
pub mod modprobe;
pub mod module;
pub mod pci;
pub mod radeon;
pub mod snd;
pub mod sys_devices;
pub mod util;

pub static DBUS_NAME: &str = "com.system76.PowerDaemon";
pub static DBUS_PATH: &str = "/com/system76/PowerDaemon";
pub static DBUS_IFACE: &str = "com.system76.PowerDaemon";

#[derive(Copy, Clone, Debug)]
pub enum Profile {
    Quiet,
    Balanced,
    Performance,
}
