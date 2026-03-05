// SPDX-License-Identifier: GPL-3.0-only

//! Application model and COSMIC Application trait implementation.

use crate::config::Config;
use crate::dbus::{self, DaemonSnapshot};
use crate::fl;
use crate::pages;
use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::{Length, Subscription};
use cosmic::prelude::*;
use cosmic::widget::{self, about::About, icon, menu, nav_bar};
use std::collections::HashMap;

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");

/// Top-level application state.
pub struct AppModel {
    /// Runtime core managed by COSMIC.
    core: cosmic::Core,
    /// Active context drawer page.
    context_page: ContextPage,
    /// About dialog content.
    about: About,
    /// Navigation bar model.
    nav: nav_bar::Model,
    /// Menu bar key bindings.
    key_binds: HashMap<menu::KeyBind, MenuAction>,
    /// Persisted configuration.
    config: Config,

    /// Whether the daemon is reachable.
    pub connected: bool,
    /// Current power profile name.
    pub profile: String,
    /// CPU temperature in degrees Celsius, if available.
    pub cpu_temp: Option<f64>,
    /// GPU temperature in degrees Celsius, if available.
    pub gpu_temp: Option<f64>,
    /// Per-channel fan state.
    pub fan_channels: Vec<FanChannel>,
    /// Active fan curves per channel: (name, [(temp_c, duty_pct)]).
    pub fan_curves: Vec<(String, Vec<(f64, f64)>)>,
    /// Whether the daemon has a fan config loaded.
    pub config_loaded: bool,
    /// Whether critical temperature state is active.
    pub critical: bool,

    /// Per-channel text input values for the override duty field.
    pub override_inputs: HashMap<String, String>,
    /// Last error from a D-Bus command, shown in the UI.
    pub error_message: Option<String>,
    /// Cached sysfs fan labels, keyed by pwm name (e.g. "pwm1" -> "CPU Fan").
    fan_labels: HashMap<String, String>,
}

/// Per-channel fan state assembled from the daemon snapshot.
#[allow(dead_code)]
pub struct FanChannel {
    /// hwmon channel name (e.g. "pwm1").
    pub name: String,
    /// Human-readable label from sysfs (e.g. "CPU Fan"), if available.
    pub label: Option<String>,
    /// Raw duty 0-255, -1 if unknown.
    pub duty_raw: i32,
    /// Current RPM, -1 if no sensor.
    pub rpm: i32,
    /// Minimum duty floor (raw 0-255), -1 if none.
    pub min_duty: i32,
    /// Whether this channel is detected as stalled.
    pub stalled: bool,
    /// Active override percentage, if set.
    pub override_pct: Option<u8>,
    /// Whether this channel is in passthrough mode.
    pub passthrough: bool,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Message {
    /// Daemon connection established.
    DaemonConnected,
    /// Daemon connection lost.
    DaemonDisconnected,
    /// Profile changed via D-Bus signal.
    ProfileChanged(String),
    /// Thermal event from D-Bus signal.
    ThermalEvent {
        /// Event type: "fallback_down", "fallback_up", or "critical".
        event_type: String,
        /// Temperature in millidegrees Celsius.
        temp_millideg: i64,
        /// Profile name after the event.
        profile: String,
    },
    /// Fan stall detected via D-Bus signal.
    StallEvent {
        /// Channel name.
        channel: String,
        /// Duty at which the stall was detected.
        duty: u8,
    },

    /// Fresh telemetry from the poll subscription.
    PollUpdate(Box<DaemonSnapshot>),
    /// Poll failed to reach the daemon.
    PollError,

    /// User requested a profile switch.
    SetProfile(String),
    /// User set a fan override.
    SetFanOverride {
        /// Channel name.
        channel: String,
        /// Duty percentage 0-100.
        duty_percent: u8,
    },
    /// User cleared a fan override.
    ClearFanOverride(String),
    /// Override text input changed.
    OverrideInputChanged {
        /// Channel name.
        channel: String,
        /// New text value.
        value: String,
    },

    /// D-Bus command completed successfully.
    CommandOk,
    /// D-Bus command failed.
    CommandError(String),

    /// Open a URL in the default browser.
    LaunchUrl(String),
    /// Toggle a context drawer page.
    ToggleContextPage(ContextPage),
    /// Config file changed on disk.
    UpdateConfig(Config),
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "com.vintagetechie.PowerCurveGui";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Set up initial state, nav bar, and about dialog.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let mut nav = nav_bar::Model::default();

        nav.insert()
            .text(fl!("overview"))
            .data::<Page>(Page::Overview)
            .icon(icon::from_name("utilities-system-monitor-symbolic"))
            .activate();

        nav.insert()
            .text(fl!("fans"))
            .data::<Page>(Page::Fans)
            .icon(icon::from_name("sensors-fan-symbolic"));

        let about = About::default()
            .name(fl!("app-title"))
            .icon(widget::icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .links([(fl!("repository"), REPOSITORY)])
            .license(env!("CARGO_PKG_LICENSE"));

        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            about,
            nav,
            key_binds: HashMap::new(),
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => config,
                })
                .unwrap_or_default(),
            connected: false,
            profile: String::new(),
            cpu_temp: None,
            gpu_temp: None,
            fan_channels: Vec::new(),
            fan_curves: Vec::new(),
            config_loaded: false,
            critical: false,
            override_inputs: HashMap::new(),
            error_message: None,
            fan_labels: read_sysfs_fan_labels(),
        };

        let command = app.update_title();
        (app, command)
    }

    /// Menu bar at the top left.
    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        let menu_bar = menu::bar(vec![menu::Tree::with_children(
            menu::root(fl!("view")).apply(Element::from),
            menu::items(
                &self.key_binds,
                vec![menu::Item::Button(fl!("about"), None, MenuAction::About)],
            ),
        )]);

        vec![menu_bar.into()]
    }

    /// Expose the nav bar model to the COSMIC runtime.
    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    /// Render the context drawer (about page).
    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::About => context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_string()),
                Message::ToggleContextPage(ContextPage::About),
            ),
        })
    }

    /// Render the active page based on nav bar selection.
    fn view(&self) -> Element<'_, Self::Message> {
        let page = self.nav.active_data::<Page>().copied().unwrap_or(Page::Overview);

        let content: Element<_> = match page {
            Page::Overview => pages::overview::view(self),
            Page::Fans => pages::fans::view(self),
        };

        widget::container(content).width(Length::Fill).height(Length::Fill).padding(20).into()
    }

    /// Register background subscriptions: config watcher, D-Bus poll, D-Bus signals.
    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch([
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
            dbus::poll_subscription(),
            dbus::signal_subscription(),
        ])
    }

    /// Handle all application messages.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::DaemonConnected => {
                self.connected = true;
                self.error_message = None;
            }

            Message::DaemonDisconnected => {
                self.connected = false;
                self.error_message = Some("Daemon not running".into());
            }

            Message::ProfileChanged(p) => {
                self.profile = p;
            }

            Message::ThermalEvent { event_type, profile, .. } => {
                self.profile = profile;
                if event_type == "critical" {
                    self.critical = true;
                }
            }

            Message::StallEvent { channel, .. } => {
                if let Some(ch) = self.fan_channels.iter_mut().find(|c| c.name == channel) {
                    ch.stalled = true;
                }
            }

            Message::PollUpdate(snap) => {
                self.connected = true;
                self.error_message = None;
                self.profile = snap.profile.clone();
                self.cpu_temp =
                    if snap.cpu_temp >= 0 { Some(snap.cpu_temp as f64 / 1000.0) } else { None };
                self.gpu_temp =
                    if snap.gpu_temp >= 0 { Some(snap.gpu_temp as f64 / 1000.0) } else { None };
                self.config_loaded = snap.config_loaded;
                self.critical = snap.critical;
                self.fan_curves = snap.curves.clone();
                self.rebuild_fan_channels(&snap);
            }

            Message::PollError => {
                self.connected = false;
                self.error_message = Some("Cannot reach daemon".into());
            }

            Message::SetProfile(name) => {
                return cosmic::task::future(async move {
                    match dbus::set_profile(&name).await {
                        Ok(()) => cosmic::Action::App(Message::CommandOk),
                        Err(e) => cosmic::Action::App(Message::CommandError(e.to_string())),
                    }
                });
            }

            Message::SetFanOverride { channel, duty_percent } => {
                return cosmic::task::future(async move {
                    match dbus::set_fan_override(&channel, duty_percent).await {
                        Ok(()) => cosmic::Action::App(Message::CommandOk),
                        Err(e) => cosmic::Action::App(Message::CommandError(e.to_string())),
                    }
                });
            }

            Message::ClearFanOverride(channel) => {
                return cosmic::task::future(async move {
                    match dbus::clear_fan_override(&channel).await {
                        Ok(()) => cosmic::Action::App(Message::CommandOk),
                        Err(e) => cosmic::Action::App(Message::CommandError(e.to_string())),
                    }
                });
            }

            Message::OverrideInputChanged { channel, value } => {
                self.override_inputs.insert(channel, value);
            }

            Message::CommandOk => {}

            Message::CommandError(e) => {
                self.error_message = Some(e);
            }

            Message::LaunchUrl(url) => {
                let _ = open::that_detached(&url);
            }

            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                }
            }

            Message::UpdateConfig(config) => {
                self.config = config;
            }
        }
        Task::none()
    }

    /// Handle nav bar page selection.
    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Self::Message>> {
        self.nav.activate(id);
        self.update_title()
    }
}

impl AppModel {
    /// Update the window title to reflect the active page.
    pub fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let mut window_title = fl!("app-title");

        if let Some(page) = self.nav.text(self.nav.active()) {
            window_title.push_str(" - ");
            window_title.push_str(page);
        }

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(window_title, id)
        } else {
            Task::none()
        }
    }

    /// Rebuild the fan channel list from a daemon snapshot.
    ///
    /// Merges duties, RPMs, floors, overrides, stall status, and passthrough
    /// into a single per-channel struct for the view to consume.
    fn rebuild_fan_channels(&mut self, snap: &DaemonSnapshot) {
        self.fan_channels = snap
            .duties
            .iter()
            .map(|(name, duty)| {
                let rpm = snap.rpms.iter().find(|(n, _)| n == name).map(|(_, r)| *r).unwrap_or(-1);
                let min_duty =
                    snap.min_duties.iter().find(|(n, _)| n == name).map(|(_, d)| *d).unwrap_or(-1);
                let stalled = snap.stalled.iter().any(|s| s == name);
                let override_pct = snap.overrides.iter().find(|(n, _)| n == name).map(|(_, p)| *p);
                let passthrough = snap.passthrough.iter().any(|p| p == name);

                let label = self.fan_labels.get(name).cloned();

                FanChannel {
                    name: name.clone(),
                    label,
                    duty_raw: *duty,
                    rpm,
                    min_duty,
                    stalled,
                    override_pct,
                    passthrough,
                }
            })
            .collect();
    }
}

/// Read fan labels from sysfs hwmon devices.
///
/// Scans all hwmon directories for fanN_label files and maps them to pwmN
/// names. Returns an empty map if no labels are found.
fn read_sysfs_fan_labels() -> HashMap<String, String> {
    let mut labels = HashMap::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") else {
        return labels;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Only consider hwmon dirs that have pwm files (fan controllers).
        if !path.join("pwm1").exists() {
            continue;
        }
        for idx in 1..=16 {
            let label_path = path.join(format!("fan{idx}_label"));
            if let Ok(raw) = std::fs::read_to_string(&label_path) {
                let label = raw.trim().to_string();
                if !label.is_empty() {
                    labels.insert(format!("pwm{idx}"), label);
                }
            }
        }
    }
    labels
}

/// Navigation pages.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum Page {
    /// Profile and temperature overview.
    #[default]
    Overview,
    /// Fan monitoring and control.
    Fans,
}

/// Context drawer pages.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    /// About this application.
    #[default]
    About,
}

/// Menu bar actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    /// Show the about dialog.
    About,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
        }
    }
}
