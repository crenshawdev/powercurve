// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

use crate::pci::PciBus;
use std::io;
use sysfs_class::{PciDevice, SysClass};

/// A discovered graphics device on the PCI bus with all its functions.
pub struct GraphicsDevice {
    functions: Vec<PciDevice>,
}

impl GraphicsDevice {
    /// Wrap an enumerated set of PCI functions as a single graphics device.
    #[must_use]
    pub fn new(functions: Vec<PciDevice>) -> Self {
        Self { functions }
    }

    /// Whether any of this device's PCI functions still exist in sysfs.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.functions.iter().any(|func| func.path().exists())
    }
}

/// Enumerates GPUs on the PCI bus, grouped by vendor.
pub struct Graphics {
    /// PCI bus handle used to trigger rescans.
    pub bus: PciBus,
    /// AMD display-class devices (vendor 0x1002).
    pub amd: Vec<GraphicsDevice>,
    /// Intel display-class devices (vendor 0x8086).
    pub intel: Vec<GraphicsDevice>,
    /// NVIDIA display-class devices (vendor 0x10DE).
    pub nvidia: Vec<GraphicsDevice>,
    /// Anything else with a display class code.
    pub other: Vec<GraphicsDevice>,
}

impl Graphics {
    /// Scans the PCI bus and categorizes all display-class devices by vendor.
    pub fn new() -> io::Result<Self> {
        let bus = PciBus::new()?;

        log::info!("Rescanning PCI bus");
        bus.rescan()?;

        let devs = PciDevice::all()?;

        let functions = |parent: &PciDevice| -> Vec<PciDevice> {
            let mut functions = Vec::new();
            if let Some(parent_slot) = parent.id().split('.').next() {
                for func in &devs {
                    if let Some(func_slot) = func.id().split('.').next()
                        && func_slot == parent_slot
                    {
                        log::info!("{}: Function for {}", func.id(), parent.id());
                        functions.push(func.clone());
                    }
                }
            }
            functions
        };

        let mut amd = Vec::new();
        let mut intel = Vec::new();
        let mut nvidia = Vec::new();
        let mut other = Vec::new();
        for dev in &devs {
            let c = dev.class()?;
            if (c >> 16) & 0xFF == 0x03 {
                match dev.vendor()? {
                    0x1002 => {
                        log::info!("{}: AMD graphics", dev.id());
                        amd.push(GraphicsDevice::new(functions(dev)));
                    }
                    0x10DE => {
                        log::info!("{}: NVIDIA graphics", dev.id());
                        nvidia.push(GraphicsDevice::new(functions(dev)));
                    }
                    0x8086 => {
                        log::info!("{}: Intel graphics", dev.id());
                        intel.push(GraphicsDevice::new(functions(dev)));
                    }
                    vendor => {
                        log::info!("{}: Other({:X}) graphics", dev.id(), vendor);
                        other.push(GraphicsDevice::new(functions(dev)));
                    }
                }
            }
        }

        Ok(Self { bus, amd, intel, nvidia, other })
    }
}
