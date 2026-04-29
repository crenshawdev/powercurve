// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

//! PowerCurve daemon and supporting libraries: power profile management,
//! configurable fan curves, GPU enumeration, and a D-Bus interface compatible
//! with `org.freedesktop.UPower.PowerProfiles` and `net.hadess.PowerProfiles`.
//!
//! The binary entry points (`daemon`, `client`, `monitor`, `watcher`,
//! `fan_detect`, `config_check`) are exposed as modules so the integration
//! examples and external test harnesses can reach them.

pub(crate) mod acpi_platform;
/// Clap-based command-line argument definitions.
pub mod args;
/// D-Bus client used by the CLI to talk to a running daemon.
pub mod client;
/// `powercurve config` subcommand: validate a fan-curve config file.
pub mod config_check;
pub(crate) mod cpufreq;
/// PowerCurve daemon: D-Bus service plus periodic fan and profile control.
pub mod daemon;
pub(crate) mod errors;
/// Fan-curve evaluation, channel discovery, and stall detection.
pub mod fan;
/// `powercurve fan-detect` subcommand: enumerate fan channels in sysfs.
pub mod fan_detect;
pub(crate) mod fan_test;
/// PCI graphics device enumeration grouped by vendor.
pub mod graphics;
pub(crate) mod kernel_parameters;
/// Stderr logger setup scoped to this crate.
pub mod logging;
/// `powercurve monitor` subcommand: print live profile and thermal events.
pub mod monitor;
/// NVIDIA GPU presence and temperature reads via `libnvidia-ml`.
pub mod nvml;
pub(crate) mod pci;
pub(crate) mod radeon;
pub(crate) mod state;
pub(crate) mod util;
/// `powercurve watcher` subcommand: per-process automatic profile switching.
pub mod watcher;

pub(crate) static DBUS_NAME: &str = "com.vintagetechie.PowerCurve";
pub(crate) static DBUS_PATH: &str = "/com/vintagetechie/PowerCurve";

#[derive(Copy, Clone, Debug)]
pub(crate) enum Profile {
    Quiet,
    Balanced,
    Performance,
}
