// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Context;
use powercurve_zbus::PowerCurveProxy;
use std::time::Duration;
use tokio::signal::unix::{SignalKind, signal};

/// How a ramp attempt ended: a spin-up floor result, or an interrupt signal.
enum RampOutcome {
    /// Lowest duty percent at which the tachometer read non-zero, if any.
    Floor(Option<u8>),
    /// SIGINT or SIGTERM arrived mid-test.
    Interrupted,
}

/// Ramp duty on a single fan channel from `start` to 100% in `step`
/// increments, reading RPM at each level. Reports the lowest duty
/// where the tachometer reads non-zero, which is the channel's spin-up
/// floor.
///
/// Uses the daemon's override mechanism so other channels keep normal
/// curve control during the test. The override is cleared on every exit
/// path — completion, error, SIGINT, or SIGTERM. (A SIGKILL skips
/// cleanup; the override then persists in the daemon until the next
/// profile change.)
pub async fn run(
    client: &mut PowerCurveProxy<'_>,
    channel: &str,
    start: u8,
    step: u8,
    settle_ms: u64,
) -> anyhow::Result<()> {
    // Validate the channel exists on the daemon.
    let duties = client
        .get_fan_duties()
        .await
        .context("failed to query fan duties (is the daemon running?)")?;
    if !duties.iter().any(|(name, _)| name == channel) {
        let known: Vec<&str> = duties.iter().map(|(n, _)| n.as_str()).collect();
        anyhow::bail!("unknown channel '{}', available: {}", channel, known.join(", "));
    }

    // Check for a tachometer on this channel.
    let rpms = client.get_fan_rpms().await.unwrap_or_default();
    let has_tacho = rpms.iter().find(|(name, _)| name == channel).is_some_and(|(_, rpm)| *rpm >= 0);
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

    println!("testing {channel} ({start}% to 100%, step {step}%, settle {settle_ms}ms)");
    println!();

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let result = ramp(client, channel, start, step, settle, &mut sigint, &mut sigterm).await;

    // Always return the channel to curve control, even when the ramp
    // errored or was interrupted. Best-effort: if the daemon is gone the
    // override died with it.
    let _ = client.clear_fan_override(channel).await;

    match result? {
        RampOutcome::Interrupted => {
            println!("\ninterrupted, override cleared");
        }
        RampOutcome::Floor(Some(pct)) => {
            println!();
            println!("{channel} spins at {pct}%");
            println!("suggested config: min_duty = {:.1}", pct as f64);
        }
        RampOutcome::Floor(None) => {
            println!();
            println!("{channel}: no spin detected up to 100%");
            println!("check that the fan is connected and the tachometer works");
        }
    }

    Ok(())
}

/// Drive the override ramp. The caller owns override cleanup so every
/// return path here — success, error, or signal — gets cleaned up the
/// same way.
async fn ramp(
    client: &PowerCurveProxy<'_>,
    channel: &str,
    start: u8,
    step: u8,
    settle: Duration,
    sigint: &mut tokio::signal::unix::Signal,
    sigterm: &mut tokio::signal::unix::Signal,
) -> anyhow::Result<RampOutcome> {
    // Stop the fan first so we start from a known state.
    client
        .set_fan_override(channel, 0)
        .await
        .map_err(|e| anyhow::anyhow!("failed to set override: {e}"))?;
    tokio::select! {
        _ = tokio::time::sleep(settle) => {}
        _ = sigint.recv() => return Ok(RampOutcome::Interrupted),
        _ = sigterm.recv() => return Ok(RampOutcome::Interrupted),
    }

    let mut floor: Option<u8> = None;
    let mut pct = start;

    while pct <= 100 {
        client
            .set_fan_override(channel, pct)
            .await
            .map_err(|e| anyhow::anyhow!("failed to set override: {e}"))?;

        // Wait for the duty to be applied and the motor to respond,
        // but also watch for signals so the caller can clean up.
        tokio::select! {
            _ = tokio::time::sleep(settle) => {}
            _ = sigint.recv() => return Ok(RampOutcome::Interrupted),
            _ = sigterm.recv() => return Ok(RampOutcome::Interrupted),
        }

        let rpm = read_channel_rpm(client, channel).await;

        match rpm {
            Some(r) => println!("  {pct}% -> {r} RPM"),
            None => println!("  {pct}% -> ? RPM"),
        }

        if rpm.is_some_and(|r| r > 0) {
            floor = Some(pct);
            break;
        }

        pct = pct.saturating_add(step);
    }

    Ok(RampOutcome::Floor(floor))
}

/// Read RPM for a specific channel from the daemon's current readings.
async fn read_channel_rpm(client: &PowerCurveProxy<'_>, channel: &str) -> Option<u32> {
    let rpms = client.get_fan_rpms().await.ok()?;
    rpms.into_iter()
        .find(|(name, _)| name == channel)
        .and_then(|(_, rpm)| if rpm >= 0 { Some(rpm as u32) } else { None })
}
