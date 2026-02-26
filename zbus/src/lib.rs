// SPDX-License-Identifier: MPL-2.0

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
}
