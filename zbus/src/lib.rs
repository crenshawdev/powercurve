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

    /// PowerProfileSwitch signal
    #[dbus_proxy(signal)]
    fn power_profile_switch(&self, profile: &str) -> zbus::Result<()>;
}
