// SPDX-License-Identifier: MPL-2.0

#[zbus::dbus_proxy(
    interface = "com.vintagetechie.PowerDaemon",
    default_service = "com.vintagetechie.PowerDaemon",
    default_path = "/com/vintagetechie/PowerDaemon"
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
