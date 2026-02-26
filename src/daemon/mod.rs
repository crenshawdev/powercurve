// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Context;
use std::{
    collections::HashMap,
    fmt::Display,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    signal::unix::{signal, SignalKind},
    sync::Mutex,
    time::sleep,
};
use zbus::Interface;

use crate::{
    errors::ProfileError,
    fan::FanDaemon,
    graphics::Graphics,
    kernel_parameters::{KernelParameter, NmiWatchdog},
    nvml::{NvmlHandle, NvidiaState},
    state,
    DBUS_NAME, DBUS_PATH,
};

mod profiles;
use self::profiles::{balanced, performance, quiet};

const NET_HADESS_POWER_PROFILES_DBUS_NAME: &str = "net.hadess.PowerProfiles";
const NET_HADESS_POWER_PROFILES_DBUS_PATH: &str = "/net/hadess/PowerProfiles";
const POWER_PROFILES_DBUS_NAME: &str = "org.freedesktop.UPower.PowerProfiles";
const POWER_PROFILES_DBUS_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

static CONTINUE: AtomicBool = AtomicBool::new(true);

async fn signal_handling() {
    let mut int = signal(SignalKind::interrupt()).unwrap();
    let mut hup = signal(SignalKind::hangup()).unwrap();
    let mut term = signal(SignalKind::terminate()).unwrap();

    let sig = tokio::select! {
        _ = int.recv() => "SIGINT",
        _ = hup.recv() => "SIGHUP",
        _ = term.recv() => "SIGTERM"
    };

    log::info!("caught signal: {}", sig);
    CONTINUE.store(false, Ordering::SeqCst);
}

// Enabled by default. Set S76_POWER_PCI_RUNTIME_PM=0 to disable if your system
// has ACPI resume issues.
static PCI_RUNTIME_PM: AtomicBool = AtomicBool::new(true);
pub(crate) fn pci_runtime_pm_support() -> bool { PCI_RUNTIME_PM.load(Ordering::SeqCst) }

struct PowerDaemon {
    power_profile:  String,
    profile_errors: Vec<ProfileError>,
    held_profiles:  Vec<(u32, &'static str, String, String)>,
    profile_ids:    u32,
    connections:    Option<(zbus::Connection, zbus::Connection, zbus::Connection)>,
}

impl PowerDaemon {
    fn new() -> Self {
        Self {
            power_profile: String::new(),
            profile_errors: Vec::new(),
            held_profiles: Vec::new(),
            profile_ids: 0,
            connections: None,
        }
    }

    async fn apply_profile(
        &mut self,
        context: &zbus::SignalContext<'_>,
        func: fn(&mut Vec<ProfileError>),
        name: &str,
    ) -> Result<(), String> {
        if self.power_profile == name {
            log::info!("profile was already set");
            return Ok(());
        }

        let _res = PowerService::power_profile_switch(context, name).await;

        func(&mut self.profile_errors);

        self.power_profile = name.into();
        state::save_profile(name);

        if self.profile_errors.is_empty() {
            Ok(())
        } else {
            let mut error_message = String::from("Errors found when setting profile:");
            for error in self.profile_errors.drain(..) {
                error_message = format!("{}\n    - {}", error_message, error);
            }

            Err(error_message)
        }
    }
}

#[derive(Clone)]
struct PowerService(Arc<Mutex<PowerDaemon>>);

impl PowerService {
    pub async fn emit_active_profile_changed(&self) {
        let (upp_connection, hadess_connection, profile) = {
            let this = self.0.lock().await;
            let Some((_, upp, hadess)) = this.connections.clone() else { return };

            let profile = profile_to_upp_str(&this.power_profile);
            (upp, hadess, profile)
        };

        let value = zvariant::Value::Str(zvariant::Str::from(profile));
        let changed = HashMap::from_iter(std::iter::once(("ActiveProfile", &value)));
        let invalidated = &[];

        if let Ok(context) = zbus::SignalContext::new(&upp_connection, POWER_PROFILES_DBUS_PATH) {
            let _res = zbus::fdo::Properties::properties_changed(
                &context,
                UPowerPowerProfiles::name(),
                &changed,
                invalidated,
            )
            .await;
        }

        if let Ok(context) =
            zbus::SignalContext::new(&hadess_connection, NET_HADESS_POWER_PROFILES_DBUS_PATH)
        {
            let _res = zbus::fdo::Properties::properties_changed(
                &context,
                NetHadessPowerProfiles::name(),
                &changed,
                invalidated,
            )
            .await;
        }
    }
}

#[zbus::dbus_interface(name = "com.vintagetechie.PowerCurve")]
impl PowerService {
    async fn quiet(
        &mut self,
        #[zbus(signal_context)] context: zbus::SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        let result = self
            .0
            .lock()
            .await
            .apply_profile(&context, quiet, "Quiet")
            .await
            .map_err(zbus_error_from_display);

        if result.is_ok() {
            self.emit_active_profile_changed().await
        }

        result
    }

    async fn balanced(
        &mut self,
        #[zbus(signal_context)] context: zbus::SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        let result = self
            .0
            .lock()
            .await
            .apply_profile(&context, balanced, "Balanced")
            .await
            .map_err(zbus_error_from_display);

        if result.is_ok() {
            self.emit_active_profile_changed().await
        }

        result
    }

    async fn performance(
        &mut self,
        #[zbus(signal_context)] context: zbus::SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        let result = self
            .0
            .lock()
            .await
            .apply_profile(&context, performance, "Performance")
            .await
            .map_err(zbus_error_from_display);

        if result.is_ok() {
            self.emit_active_profile_changed().await
        }

        result
    }

    #[dbus_interface(out_args("profile"))]
    async fn get_profile(&self) -> zbus::fdo::Result<String> {
        Ok(self.0.lock().await.power_profile.clone())
    }

    #[dbus_interface(signal)]
    async fn power_profile_switch(
        context: &zbus::SignalContext<'_>,
        profile: &str,
    ) -> zbus::Result<()>;
}

struct UPowerPowerProfiles(Arc<Mutex<PowerDaemon>>);

impl UPowerPowerProfiles {
    pub async fn apply_held_profile(&mut self) {
        let mut set_profile = "balanced";

        for (_, profile, ..) in &self.0.lock().await.held_profiles {
            match *profile {
                "power-saver" => {
                    set_profile = "power-saver";
                    break;
                }
                "performance" => set_profile = "performance",
                _ => (),
            }
        }

        self.set_active_profile(set_profile).await;
    }
}

#[zbus::dbus_interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl UPowerPowerProfiles {
    #[dbus_interface(out_args("cookie"))]
    async fn hold_profile(
        &mut self,
        profile: &str,
        reason: &str,
        application_id: &str,
    ) -> zbus::fdo::Result<u32> {
        let mut this = self.0.lock().await;
        let id = this.profile_ids;

        let profile_static = match profile {
            "power-saver" => "power-saver",
            "balanced" => "balanced",
            "performance" => "performance",
            _ => return Err(zbus::fdo::Error::Failed(String::from("unknown power profile"))),
        };

        this.profile_ids += 1;
        this.held_profiles.push((id, profile_static, reason.into(), application_id.into()));
        drop(this);

        self.apply_held_profile().await;

        Ok(id)
    }

    async fn release_profile(&mut self, cookie: u32) {
        let mut this = self.0.lock().await;

        if let Some(pos) = this.held_profiles.iter().position(|(id, ..)| *id == cookie) {
            this.held_profiles.swap_remove(pos);
            drop(this);

            self.apply_held_profile().await;

            let this = self.0.lock().await;
            let Some((_, ref connection, _)) = this.connections else {
                return;
            };

            if let Ok(context) = zbus::SignalContext::new(connection, POWER_PROFILES_DBUS_PATH) {
                let _res = Self::profile_released(&context, cookie);
            }
        }
    }

    #[dbus_interface(signal)]
    async fn profile_released(context: &zbus::SignalContext<'_>, cookie: u32) -> zbus::Result<()>;

    #[dbus_interface(property)]
    async fn active_profile(&self) -> &str {
        profile_to_upp_str(self.0.lock().await.power_profile.as_str())
    }

    #[dbus_interface(property)]
    async fn set_active_profile(&mut self, profile: &str) {
        let (func, profile): (fn(&mut Vec<ProfileError>), &'static str) = match profile {
            "power-saver" => (quiet, "Quiet"),
            "balanced" => (balanced, "Balanced"),
            "performance" => (performance, "Performance"),
            _ => return,
        };

        let mut this = self.0.lock().await;
        let Some((ref connection, ..)) = this.connections else {
            return;
        };

        if let Ok(context) = zbus::SignalContext::new(connection, DBUS_PATH) {
            let _res =
                this.apply_profile(&context, func, profile).await.map_err(zbus_error_from_display);
        }
    }

    #[dbus_interface(property)]
    async fn profiles(&self) -> Vec<HashMap<&'static str, zvariant::Value<'_>>> {
        vec![
            {
                let mut map = HashMap::new();
                map.insert("Profile", zvariant::Value::Str(zvariant::Str::from("balanced")));
                map
            },
            {
                let mut map = HashMap::new();
                map.insert("Profile", zvariant::Value::Str(zvariant::Str::from("performance")));
                map
            },
            {
                let mut map = HashMap::new();
                map.insert("Profile", zvariant::Value::Str(zvariant::Str::from("power-saver")));
                map
            },
        ]
    }

    #[dbus_interface(property)]
    async fn performance_degraded(&self) -> &str { "" }

    #[dbus_interface(property)]
    async fn performance_inhibited(&self) -> &str { "" }

    #[dbus_interface(property)]
    async fn active_profile_holds(&self) -> Vec<HashMap<String, zvariant::Value<'_>>> { Vec::new() }

    #[dbus_interface(property)]
    async fn actions(&self) -> Vec<String> { vec![] }

    #[dbus_interface(property)]
    async fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }
}

pub struct NetHadessPowerProfiles(UPowerPowerProfiles);

#[zbus::dbus_interface(name = "net.hadess.PowerProfiles")]
impl NetHadessPowerProfiles {
    #[dbus_interface(property)]
    async fn active_profile(&self) -> &str { self.0.active_profile().await }

    #[dbus_interface(property)]
    async fn set_active_profile(&mut self, profile: &str) {
        self.0.set_active_profile(profile).await
    }

    #[dbus_interface(property)]
    async fn performance_inhibited(&self) -> &str { self.0.performance_inhibited().await }

    #[dbus_interface(property)]
    async fn profiles(&self) -> Vec<HashMap<&'static str, zvariant::Value<'_>>> {
        self.0.profiles().await
    }

    #[dbus_interface(property)]
    async fn actions(&self) -> Vec<String> { self.0.actions().await }
}

#[tokio::main(flavor = "current_thread")]
pub async fn daemon() -> anyhow::Result<()> {
    let signal_handling_fut = signal_handling();

    let pci_runtime_pm = std::env::var("S76_POWER_PCI_RUNTIME_PM")
        .map(|v| v != "0")
        .unwrap_or(true);

    PCI_RUNTIME_PM.store(pci_runtime_pm, Ordering::SeqCst);

    let graphics = Graphics::new()?;
    let nvidia_state = if graphics.nvidia.is_empty() {
        NvidiaState::Absent
    } else {
        match NvmlHandle::open() {
            Some(handle) => {
                log::info!("nvml: loaded, {} device(s)", handle.device_count());
                NvidiaState::Active(handle)
            }
            None => {
                log::warn!("nvidia GPU detected but NVML unavailable, GPU fans will run at max");
                NvidiaState::Unavailable
            }
        }
    };

    NmiWatchdog.set(b"0");

    let daemon = Arc::new(Mutex::new(PowerDaemon::new()));
    let mut power_service = PowerService(daemon.clone());

    // powerprofilesctl
    let upp_connection = connect_dbus(
        POWER_PROFILES_DBUS_NAME,
        POWER_PROFILES_DBUS_PATH,
        || UPowerPowerProfiles(daemon.clone()),
    )
    .await?;

    // gnome-shell
    let hadess_connection = connect_dbus(
        NET_HADESS_POWER_PROFILES_DBUS_NAME,
        NET_HADESS_POWER_PROFILES_DBUS_PATH,
        || NetHadessPowerProfiles(UPowerPowerProfiles(daemon.clone())),
    )
    .await?;

    let power_service_clone = power_service.clone();
    let connection = connect_dbus(DBUS_NAME, DBUS_PATH, || power_service_clone.clone()).await?;

    power_service.0.lock().await.connections =
        Some((connection.clone(), upp_connection, hadess_connection));

    let context = zbus::SignalContext::new(&connection, DBUS_PATH)
        .context("unable to create signal context")?;

    let initial_profile = state::load_profile().unwrap_or_else(|| String::from("Balanced"));
    log::info!("restoring profile: {}", initial_profile);

    let init_result = match initial_profile.as_str() {
        "Quiet" => power_service.quiet(context.clone()).await,
        "Performance" => power_service.performance(context.clone()).await,
        _ => power_service.balanced(context.clone()).await,
    };

    if let Err(why) = init_result {
        log::warn!("failed to set initial profile: {}", why);
    }

    let mut fan_daemon = FanDaemon::new(nvidia_state);

    let main_loop = async move {
        while CONTINUE.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(1000)).await;
            fan_daemon.step();
        }
    };

    log::info!("Handling dbus requests");
    futures_lite::future::zip(signal_handling_fut, main_loop).await;

    log::info!("daemon exited from loop");
    Ok(())
}

fn profile_to_upp_str(system76_profile: &str) -> &'static str {
    match system76_profile {
        "Quiet" => "power-saver",
        "Balanced" => "balanced",
        "Performance" => "performance",
        _ => "unknown",
    }
}

fn zbus_error_from_display<E: Display>(why: E) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(format!("{}", why))
}

const MAX_DBUS_RETRIES: u32 = 5;

/// Connect to the system bus with retry and exponential backoff.
///
/// Bus name acquisition can fail if another process holds the name. This
/// retries a few times before giving up, which handles transient races
/// during service restarts.
async fn connect_dbus<I, F>(
    bus_name: &'static str,
    path: &'static str,
    make_iface: F,
) -> anyhow::Result<zbus::Connection>
where
    I: zbus::Interface,
    F: Fn() -> I,
{
    let mut last_err = None;

    for attempt in 1..=MAX_DBUS_RETRIES {
        let result = zbus::ConnectionBuilder::system()
            .context("failed to create zbus connection builder")?
            .name(bus_name)
            .context("unable to register name")?
            .serve_at(path, make_iface())
            .context("unable to serve")?
            .build()
            .await;

        match result {
            Ok(conn) => return Ok(conn),
            Err(e) => {
                if attempt < MAX_DBUS_RETRIES {
                    let delay = Duration::from_millis(500 * 2u64.pow(attempt - 1));
                    log::warn!(
                        "{}: attempt {}/{} failed ({}), retrying in {}ms",
                        bus_name,
                        attempt,
                        MAX_DBUS_RETRIES,
                        e,
                        delay.as_millis(),
                    );
                    sleep(delay).await;
                }
                last_err = Some(e);
            }
        }
    }

    Err(anyhow::anyhow!(
        "failed to acquire {} after {} attempts, check if another instance is running: {}",
        bus_name,
        MAX_DBUS_RETRIES,
        last_err.unwrap(),
    ))
}
