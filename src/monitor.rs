// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Context;
use futures_lite::StreamExt;
use powercurve_zbus::PowerCurveProxy;
use zbus::Connection;

/// Run a long-lived monitor that listens for profile and thermal events
/// on the system bus and sends desktop notifications via the session bus.
#[tokio::main(flavor = "current_thread")]
pub async fn run() -> anyhow::Result<()> {
    // Ignore SIGHUP so `kill -HUP $(pidof powercurve)` only reloads
    // the daemon without killing the monitor.
    unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN); }
    let system = Connection::system()
        .await
        .context("failed to connect to system bus")?;

    let session = Connection::session()
        .await
        .context("failed to connect to session bus")?;

    let proxy = PowerCurveProxy::new(&system)
        .await
        .context("failed to connect to powercurve daemon")?;

    let mut profile_stream = proxy.receive_power_profile_switch().await?;
    let mut thermal_stream = proxy.receive_thermal_event().await?;

    println!("monitoring powercurve events (ctrl-c to stop)");

    loop {
        tokio::select! {
            Some(signal) = profile_stream.next() => {
                if let Ok(args) = signal.args() {
                    let profile = args.profile();
                    println!("profile: {}", profile);
                    let _ = notify(
                        &session,
                        "PowerCurve",
                        &format!("Profile switched to {}", profile),
                    ).await;
                }
            }
            Some(signal) = thermal_stream.next() => {
                if let Ok(args) = signal.args() {
                    let event = *args.event_type();
                    let temp = *args.temp_millideg();
                    let profile = args.profile();

                    let (summary, body) = match event {
                        "fallback_down" => (
                            "Thermal Fallback",
                            format!(
                                "High temperature ({:.1}C), downshifted to {}",
                                temp as f64 / 1000.0,
                                profile,
                            ),
                        ),
                        "fallback_up" => (
                            "Thermal Recovery",
                            format!("Temps stable, restored profile to {}", profile),
                        ),
                        "critical" => (
                            "Critical Temperature",
                            format!(
                                "Temperature at {:.1}C, all fans at max",
                                temp as f64 / 1000.0,
                            ),
                        ),
                        _ => ("PowerCurve", format!("{}: {}", event, profile)),
                    };

                    println!("{}: {}", summary, body);
                    let _ = notify(&session, summary, &body).await;
                }
            }
            else => break,
        }
    }

    Ok(())
}

/// Send a desktop notification via org.freedesktop.Notifications on the session bus.
async fn notify(session: &Connection, summary: &str, body: &str) -> anyhow::Result<()> {
    session.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &(
            "powercurve",                                    // app_name
            0u32,                                            // replaces_id
            "",                                              // app_icon
            summary,                                         // summary
            body,                                            // body
            Vec::<String>::new(),                             // actions
            std::collections::HashMap::<String, zvariant::Value>::new(), // hints
            5000i32,                                         // expire_timeout ms
        ),
    ).await.context("failed to send notification")?;

    Ok(())
}
