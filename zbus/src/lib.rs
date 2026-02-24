// SPDX-License-Identifier: MPL-2.0

#[zbus::dbus_proxy(
    interface = "com.system76.PowerDaemon",
    default_service = "com.system76.PowerDaemon",
    default_path = "/com/system76/PowerDaemon"
)]
trait PowerDaemon {
    /// Quiet method
    fn quiet(&self) -> zbus::Result<()>;

    /// Balanced method
    fn balanced(&self) -> zbus::Result<()>;

    /// Performance method
    fn performance(&self) -> zbus::Result<()>;

    /// GetProfile method
    fn get_profile(&self) -> zbus::Result<String>;

    /// PowerProfileSwitch signal
    #[dbus_proxy(signal)]
    fn power_profile_switch(&self, profile: &str) -> zbus::Result<()>;
}
