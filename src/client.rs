// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

use crate::args::Args;
use anyhow::Context;
use intel_pstate::PState;
use std::io;
use system76_power_zbus::PowerDaemonProxy;

async fn profile(client: &mut PowerDaemonProxy<'_>) -> io::Result<()> {
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

    let mut client = PowerDaemonProxy::new(&connection)
        .await
        .context("failed to connect to system76-power daemon")?;

    match args {
        Args::Profile { profile: name } => match name.as_deref() {
            Some("balanced") => client.balanced().await.map_err(zbus_error),
            Some("quiet") => client.quiet().await.map_err(zbus_error),
            Some("performance") => client.performance().await.map_err(zbus_error),
            _ => profile(&mut client).await.context("failed to get power profile"),
        },
        Args::Daemon { .. } => unreachable!(),
    }
}

fn zbus_error(why: zbus::Error) -> anyhow::Error { anyhow::anyhow!("{}", why) }
