// SPDX-License-Identifier: GPL-3.0-only

//! D-Bus integration for the GUI.
//!
//! Bridges the async powercurve daemon interface into cosmic/iced subscriptions
//! and one-shot commands. Two subscriptions run concurrently: one listens for
//! D-Bus signals (profile changes, thermal events, fan stalls) and another polls
//! the daemon on a timer for continuous telemetry.

use crate::app::Message;
use cosmic::iced::Subscription;
use cosmic::iced_futures;
use futures_lite::StreamExt;
use futures_util::SinkExt;
use powercurve_zbus::PowerCurveProxy;
use std::time::Duration;

/// Snapshot of the daemon's current state, fetched by polling.
#[derive(Debug, Clone)]
pub struct DaemonSnapshot {
    /// Current power profile name.
    pub profile: String,
    /// CPU temperature in millidegrees, -1 if unavailable.
    pub cpu_temp: i64,
    /// GPU temperature in millidegrees, -1 if unavailable.
    pub gpu_temp: i64,
    /// Per-channel duty values (0-255), -1 if unknown.
    pub duties: Vec<(String, i32)>,
    /// Per-channel RPM readings, -1 if no sensor.
    pub rpms: Vec<(String, i32)>,
    /// Per-channel minimum duty floors, -1 if none.
    pub min_duties: Vec<(String, i32)>,
    /// Active fan overrides as (channel, duty_percent).
    pub overrides: Vec<(String, u8)>,
    /// Names of stalled fan channels.
    pub stalled: Vec<String>,
    /// Names of passthrough channels.
    pub passthrough: Vec<String>,
    /// Active fan curves per channel.
    pub curves: Vec<(String, Vec<(f64, f64)>)>,
    /// Whether the daemon has loaded a fan config.
    pub config_loaded: bool,
    /// Whether the daemon is in critical temperature state.
    pub critical: bool,
}

/// Subscription that listens for D-Bus signals from the daemon.
///
/// Connects to the system bus, subscribes to profile, thermal, and stall
/// signals, and forwards them as application messages. Reconnects on failure.
pub fn signal_subscription() -> Subscription<Message> {
    Subscription::run(|| {
        iced_futures::stream::channel(16, |mut tx| async move {
            loop {
                let result: Result<(), Box<dyn std::error::Error + Send + Sync>> = async {
                    let conn = zbus::Connection::system().await?;
                    let proxy = PowerCurveProxy::new(&conn).await?;
                    let _ = tx.send(Message::DaemonConnected).await;

                    let mut profile_stream = proxy.receive_power_profile_switch().await?;
                    let mut thermal_stream = proxy.receive_thermal_event().await?;
                    let mut stall_stream = proxy.receive_stall_event().await?;

                    loop {
                        tokio::select! {
                            Some(sig) = profile_stream.next() => {
                                if let Ok(args) = sig.args() {
                                    let _ = tx.send(Message::ProfileChanged(
                                        args.profile().to_string(),
                                    )).await;
                                }
                            }
                            Some(sig) = thermal_stream.next() => {
                                if let Ok(args) = sig.args() {
                                    let _ = tx.send(Message::ThermalEvent {
                                        event_type: args.event_type().to_string(),
                                        temp_millideg: *args.temp_millideg(),
                                        profile: args.profile().to_string(),
                                    }).await;
                                }
                            }
                            Some(sig) = stall_stream.next() => {
                                if let Ok(args) = sig.args() {
                                    let _ = tx.send(Message::StallEvent {
                                        channel: args.channel().to_string(),
                                        duty: *args.duty(),
                                    }).await;
                                }
                            }
                            else => break,
                        }
                    }

                    Ok(())
                }.await;

                if result.is_err() {
                    let _ = tx.send(Message::DaemonDisconnected).await;
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        })
    })
}

/// Subscription that polls the daemon every 2 seconds for telemetry.
pub fn poll_subscription() -> Subscription<Message> {
    Subscription::run(|| {
        iced_futures::stream::channel(4, |mut tx| async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut interval = tokio::time::interval(Duration::from_secs(2));

            loop {
                interval.tick().await;
                let msg = match poll_daemon().await {
                    Ok(snap) => Message::PollUpdate(Box::new(snap)),
                    Err(_) => Message::PollError,
                };
                let _ = tx.send(msg).await;
            }
        })
    })
}

/// Fetch the full daemon state in a single pass.
async fn poll_daemon() -> Result<DaemonSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let conn = zbus::Connection::system().await?;
    let proxy = PowerCurveProxy::new(&conn).await?;

    let profile = proxy.get_profile().await?;
    let (cpu_temp, gpu_temp) = proxy.get_temperatures().await?;
    let duties = proxy.get_fan_duties().await.unwrap_or_default();
    let rpms = proxy.get_fan_rpms().await.unwrap_or_default();
    let min_duties = proxy.get_fan_min_duties().await.unwrap_or_default();
    let overrides = proxy.get_fan_overrides().await.unwrap_or_default();
    let stalled = proxy.get_stalled_fans().await.unwrap_or_default();
    let passthrough = proxy.get_passthrough_channels().await.unwrap_or_default();
    let curves = proxy.get_fan_curves().await.unwrap_or_default();
    let (config_loaded, critical) = proxy.get_fan_config_status().await.unwrap_or((false, false));

    Ok(DaemonSnapshot {
        profile,
        cpu_temp,
        gpu_temp,
        duties,
        rpms,
        min_duties,
        overrides,
        stalled,
        passthrough,
        curves,
        config_loaded,
        critical,
    })
}

/// Switch the daemon to the named power profile.
pub async fn set_profile(name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conn = zbus::Connection::system().await?;
    let proxy = PowerCurveProxy::new(&conn).await?;
    match name {
        "Quiet" => proxy.quiet().await?,
        "Balanced" => proxy.balanced().await?,
        "Performance" => proxy.performance().await?,
        _ => {}
    }
    Ok(())
}

/// Set a temporary fan override on a channel.
pub async fn set_fan_override(
    channel: &str,
    duty_percent: u8,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conn = zbus::Connection::system().await?;
    let proxy = PowerCurveProxy::new(&conn).await?;
    proxy.set_fan_override(channel, duty_percent).await?;
    Ok(())
}

/// Clear a temporary fan override on a channel.
pub async fn clear_fan_override(
    channel: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let conn = zbus::Connection::system().await?;
    let proxy = PowerCurveProxy::new(&conn).await?;
    proxy.clear_fan_override(channel).await?;
    Ok(())
}
