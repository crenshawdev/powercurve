// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

use crate::args::Args;
use anyhow::Context;
use intel_pstate::PState;
use powercurve_zbus::PowerCurveProxy;
use std::io;

async fn profile(client: &mut PowerCurveProxy<'_>) -> io::Result<()> {
    let profile = client.get_profile().await.ok();
    let profile = profile.as_ref().map_or("?", |s| s.as_str());
    println!("Power Profile: {}", profile);

    if let Ok(values) = PState::new().and_then(|pstate| pstate.values()) {
        println!(
            "CPU: {}% - {}%, {}",
            values.min_perf_pct,
            values.max_perf_pct,
            if values.no_turbo { "No Turbo" } else { "Turbo" }
        );
    }

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
pub async fn client(args: &Args) -> anyhow::Result<()> {
    let connection =
        zbus::Connection::system().await.context("failed to create zbus system connection")?;

    let mut client = PowerCurveProxy::new(&connection)
        .await
        .context("failed to connect to powercurve daemon")?;

    match args {
        Args::Profile { profile: name } => match name.as_deref() {
            Some("balanced") => client.balanced().await.map_err(zbus_error),
            Some("quiet") => client.quiet().await.map_err(zbus_error),
            Some("performance") => client.performance().await.map_err(zbus_error),
            _ => profile(&mut client).await.context("failed to get power profile"),
        },
        Args::Status => status(&mut client).await.context("failed to get daemon status"),
        Args::Fan { channel, duty } => fan_override(&mut client, channel, duty).await,
        Args::FanTest { channel, step, start, settle } => {
            crate::fan_test::run(&mut client, channel, *start, *step, *settle).await
        }
        Args::Daemon { .. }
        | Args::FanDetect { .. }
        | Args::Config
        | Args::Monitor
        | Args::Watch
        | Args::Version => {
            unreachable!("variant dispatched in main before reaching client")
        }
    }
}

/// Display current daemon state: profile, temps, fan duties, and active curves.
async fn status(client: &mut PowerCurveProxy<'_>) -> io::Result<()> {
    let profile = client.get_profile().await.ok();
    let profile = profile.as_ref().map_or("?", |s| s.as_str());
    println!("Profile: {}", profile);

    if let Ok((cpu, gpu)) = client.get_temperatures().await {
        if cpu >= 0 {
            println!("CPU:     {:.1}C", cpu as f64 / 1000.0);
        }
        if gpu >= 0 {
            println!("GPU:     {:.1}C", gpu as f64 / 1000.0);
        }
    }

    let overrides: Vec<(String, u8)> = client.get_fan_overrides().await.unwrap_or_default();
    let floors: Vec<(String, i32)> = client.get_fan_min_duties().await.unwrap_or_default();
    let rpms: Vec<(String, i32)> = client.get_fan_rpms().await.unwrap_or_default();
    let stalled: Vec<String> = client.get_stalled_fans().await.unwrap_or_default();
    let passthrough: Vec<String> = client.get_passthrough_channels().await.unwrap_or_default();

    if let Ok(duties) = client.get_fan_duties().await {
        for (name, duty) in &duties {
            if passthrough.iter().any(|p| p == name) {
                println!("{}: [passthrough]", name);
                continue;
            }
            let override_tag = overrides
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, pct)| format!(" [override {}%]", pct))
                .unwrap_or_default();
            let floor_tag = floors
                .iter()
                .find(|(k, _)| k == name)
                .and_then(|(_, d)| {
                    if *d >= 0 {
                        Some(format!(" [floor {:.0}%]", (*d as f64 / 255.0) * 100.0))
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let rpm_tag = rpms
                .iter()
                .find(|(k, _)| k == name)
                .and_then(|(_, r)| if *r >= 0 { Some(format!(" [{} RPM]", r)) } else { None })
                .unwrap_or_default();
            let stall_tag = if stalled.iter().any(|s| s == name) { " [STALLED]" } else { "" };
            if *duty >= 0 {
                let pct = (*duty as f64 / 255.0) * 100.0;
                println!(
                    "{}: {}/255 ({:.0}%){}{}{}{}",
                    name, duty, pct, override_tag, floor_tag, rpm_tag, stall_tag
                );
            } else {
                println!("{}: --{}{}{}{}", name, override_tag, floor_tag, rpm_tag, stall_tag);
            }
        }
    }

    if let Ok(curves) = client.get_fan_curves().await
        && !curves.is_empty()
    {
        println!("\nCurves:");
        for (name, points) in &curves {
            let pts: Vec<String> =
                points.iter().map(|(t, d)| format!("{:.0}C/{:.0}%", t, d)).collect();
            println!("  {}: {}", name, pts.join(" "));
        }
    }

    if let Ok((config_loaded, critical)) = client.get_fan_config_status().await {
        if !config_loaded {
            println!("\nfan config not loaded");
        }
        if critical {
            println!("\n!! CRITICAL TEMPERATURE !!");
        }
    }

    Ok(())
}

/// Set or clear a temporary fan duty override.
async fn fan_override(
    client: &mut PowerCurveProxy<'_>,
    channel: &str,
    duty: &str,
) -> anyhow::Result<()> {
    if duty == "clear" {
        client.clear_fan_override(channel).await.map_err(zbus_error)?;
        println!("{}: override cleared", channel);
    } else {
        let pct: u8 = duty.parse().context("duty must be 0-100 or 'clear'")?;
        if pct > 100 {
            anyhow::bail!("duty must be 0-100");
        }
        client.set_fan_override(channel, pct).await.map_err(zbus_error)?;
        println!("{}: override set to {}%", channel, pct);
    }
    Ok(())
}

fn zbus_error(why: zbus::Error) -> anyhow::Error {
    anyhow::anyhow!("{}", why)
}
