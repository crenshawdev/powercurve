// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Context;
use powercurve_zbus::PowerCurveProxy;
use std::time::Duration;
use tokio::signal::unix::{signal, SignalKind};

/// Ramp duty on a single fan channel from `start` to 100% in `step`
/// increments, reading RPM at each level. Reports the lowest duty
/// where the tachometer reads non-zero, which is the channel's spin-up
/// floor.
///
/// Uses the daemon's override mechanism so other channels keep normal
/// curve control during the test. The override is cleared on
/// completion, error, or Ctrl-C.
pub async fn run(
    client: &mut PowerCurveProxy<'_>,
    channel: &str,
    start: u8,
    step: u8,
    settle_ms: u64,
) -> anyhow::Result<()> {
    // Validate the channel exists on the daemon.
    let duties = client.get_fan_duties().await
        .context("failed to query fan duties (is the daemon running?)")?;
    if !duties.iter().any(|(name, _)| name == channel) {
        let known: Vec<&str> = duties.iter().map(|(n, _)| n.as_str()).collect();
        anyhow::bail!("unknown channel '{}', available: {}", channel, known.join(", "));
    }

    // Check for a tachometer on this channel.
    let rpms = client.get_fan_rpms().await.unwrap_or_default();
    let has_tacho = rpms.iter()
        .find(|(name, _)| name == channel)
        .is_some_and(|(_, rpm)| *rpm >= 0);
    if !has_tacho {
        anyhow::bail!(
            "{} has no tachometer (RPM reads -1). Enable stall_detect in fan.toml \
             or check that fan{}_input exists in sysfs",
            channel,
            channel.strip_prefix("pwm").unwrap_or("?"),
        );
    }

    let step = step.clamp(1, 50);
    let start = start.min(99);
    let settle = Duration::from_millis(settle_ms.max(500));

    println!("testing {} ({}% to 100%, step {}%, settle {}ms)",
        channel, start, step, settle_ms);
    println!();

    // Install Ctrl-C handler that clears the override before exiting.
    let client_channel = channel.to_string();
    let mut sigint = signal(SignalKind::interrupt())?;

    // Stop the fan first so we start from a known state.
    client.set_fan_override(channel, 0).await
        .map_err(|e| anyhow::anyhow!("failed to set override: {}", e))?;
    tokio::time::sleep(settle).await;

    let mut floor: Option<u8> = None;
    let mut pct = start;

    loop {
        if pct > 100 { break; }

        client.set_fan_override(channel, pct).await
            .map_err(|e| anyhow::anyhow!("failed to set override: {}", e))?;

        // Wait for the duty to be applied and the motor to respond,
        // but also watch for Ctrl-C so we can clean up.
        tokio::select! {
            _ = tokio::time::sleep(settle) => {}
            _ = sigint.recv() => {
                println!("\ninterrupted, clearing override");
                let _ = client.clear_fan_override(&client_channel).await;
                return Ok(());
            }
        }

        let rpm = read_channel_rpm(client, channel).await;

        match rpm {
            Some(r) => println!("  {}% -> {} RPM", pct, r),
            None => println!("  {}% -> ? RPM", pct),
        }

        if rpm.is_some_and(|r| r > 0) {
            floor = Some(pct);
            break;
        }

        pct = pct.saturating_add(step);
    }

    // Clear override and return to curve control.
    let _ = client.clear_fan_override(channel).await;

    println!();
    match floor {
        Some(pct) => {
            println!("{} spins at {}%", channel, pct);
            println!("suggested config: min_duty = {:.1}", pct as f64);
        }
        None => {
            println!("{}: no spin detected up to 100%", channel);
            println!("check that the fan is connected and the tachometer works");
        }
    }

    Ok(())
}

/// Read RPM for a specific channel from the daemon's current readings.
async fn read_channel_rpm(
    client: &PowerCurveProxy<'_>,
    channel: &str,
) -> Option<u32> {
    let rpms = client.get_fan_rpms().await.ok()?;
    rpms.into_iter()
        .find(|(name, _)| name == channel)
        .and_then(|(_, rpm)| if rpm >= 0 { Some(rpm as u32) } else { None })
}
