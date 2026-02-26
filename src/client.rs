// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

use crate::args::Args;
use anyhow::Context;
use intel_pstate::PState;
use std::io;
use powercurve_zbus::PowerCurveProxy;

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
        Args::Daemon { .. } | Args::FanDetect { .. } | Args::Config | Args::Monitor => {
            unreachable!()
        }
    }
}

/// Display current daemon state: profile, temps, and fan duties.
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

    if let Ok(duties) = client.get_fan_duties().await {
        for (name, duty) in &duties {
            if *duty >= 0 {
                let pct = (*duty as f64 / 255.0) * 100.0;
                println!("{}: {}/255 ({:.0}%)", name, duty, pct);
            } else {
                println!("{}: --", name);
            }
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

fn zbus_error(why: zbus::Error) -> anyhow::Error { anyhow::anyhow!("{}", why) }
