// SPDX-License-Identifier: MPL-2.0

//! Generated client proxy for the `com.vintagetechie.PowerCurve` D-Bus
//! service. The `PowerCurveProxy` type is produced by the `dbus_proxy` macro
//! from the trait below and is the only public item this crate exposes.

#![allow(clippy::type_complexity)]
// The dbus_proxy macro emits a `PowerCurveProxy` struct with a private inner
// field and a constructor that the lint cannot see doc comments on. Allow the
// generated scaffolding at crate scope; the per-method docs come through fine.
#![allow(missing_docs)]

/// D-Bus client proxy for the PowerCurve daemon.
#[zbus::dbus_proxy(
    interface = "com.vintagetechie.PowerCurve",
    default_service = "com.vintagetechie.PowerCurve",
    default_path = "/com/vintagetechie/PowerCurve"
)]
trait PowerCurve {
    /// Quiet method
    fn quiet(&self) -> zbus::Result<()>;

    /// Balanced method
    fn balanced(&self) -> zbus::Result<()>;

    /// Performance method
    fn performance(&self) -> zbus::Result<()>;

    /// GetProfile method
    fn get_profile(&self) -> zbus::Result<String>;

    /// Get CPU and GPU temperatures in millidegrees.
    fn get_temperatures(&self) -> zbus::Result<(i64, i64)>;

    /// Get current fan duties as (channel_name, duty_byte) pairs.
    fn get_fan_duties(&self) -> zbus::Result<Vec<(String, i32)>>;

    /// Get fan config status: (config_loaded, critical).
    fn get_fan_config_status(&self) -> zbus::Result<(bool, bool)>;

    /// Get the active fan curve for each channel as (name, [(temp_c, duty_pct)]) pairs.
    fn get_fan_curves(&self) -> zbus::Result<Vec<(String, Vec<(f64, f64)>)>>;

    /// Get currently active fan overrides as (channel, duty_percent) pairs.
    fn get_fan_overrides(&self) -> zbus::Result<Vec<(String, u8)>>;

    /// Temporarily override a fan channel's duty cycle.
    fn set_fan_override(&self, channel: &str, duty_percent: u8) -> zbus::Result<()>;

    /// Clear a temporary fan override.
    fn clear_fan_override(&self, channel: &str) -> zbus::Result<()>;

    /// Get minimum duty floors as (channel_name, duty_byte) pairs. -1 = no floor.
    fn get_fan_min_duties(&self) -> zbus::Result<Vec<(String, i32)>>;

    /// Get current RPM readings as (channel_name, rpm) pairs. -1 = no sensor.
    fn get_fan_rpms(&self) -> zbus::Result<Vec<(String, i32)>>;

    /// Get names of channels in passthrough mode.
    fn get_passthrough_channels(&self) -> zbus::Result<Vec<String>>;

    /// Get names of channels currently detected as stalled.
    fn get_stalled_fans(&self) -> zbus::Result<Vec<String>>;

    /// PowerProfileSwitch signal
    #[dbus_proxy(signal)]
    fn power_profile_switch(&self, profile: &str) -> zbus::Result<()>;

    /// ThermalEvent signal
    #[dbus_proxy(signal)]
    fn thermal_event(
        &self,
        event_type: &str,
        temp_millideg: i64,
        profile: &str,
    ) -> zbus::Result<()>;

    /// StallEvent signal, emitted when a fan channel stalls.
    #[dbus_proxy(signal)]
    fn stall_event(&self, channel: &str, duty: u8) -> zbus::Result<()>;
}
