// SPDX-License-Identifier: GPL-3.0-only

use intel_pstate::PStateError;
use std::io;

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("failed to set pci device profiles: {0}")]
    PciDevice(#[from] PciDeviceError),
    #[error("failed to set pstate profiles: {0}")]
    PState(#[from] PStateError),
    #[error("failed to set scsi host profiles: {0}")]
    ScsiHost(#[from] ScsiHostError),
}

#[derive(Debug, thiserror::Error)]
pub enum PciDeviceError {
    #[error("failed to set PCI device runtime PM on {}: {}", _0, _1)]
    SetRuntimePm(String, io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum ScsiHostError {
    #[error("failed to set link time power management policy {} on {}: {}", _0, _1, _2)]
    LinkTimePolicy(&'static str, String, io::Error),
}
