// Copyright 2018-2021 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

//! `powercurve` binary: parses CLI args and dispatches to the daemon, monitor,
//! watcher, fan tooling, or D-Bus client based on the chosen subcommand.

#![deny(clippy::all)]

use clap::Parser;
use log::LevelFilter;
use powercurve::{args::Args, client, config_check, daemon, fan_detect, logging, monitor, watcher};
use std::process;

fn main() {
    let args = Args::parse();

    let res = match args {
        Args::Daemon { quiet, verbose } => {
            if let Err(why) = logging::setup(if verbose {
                LevelFilter::Debug
            } else if quiet {
                LevelFilter::Off
            } else {
                LevelFilter::Info
            }) {
                eprintln!("failed to set up logging: {why}");
                process::exit(1);
            }

            if unsafe { libc::geteuid() } == 0 {
                daemon::daemon()
            } else {
                Err(anyhow::anyhow!("must be run as root"))
            }
        }
        Args::Version => {
            println!("powercurve {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Args::FanDetect { generate } => fan_detect::run(generate),
        Args::Config => config_check::run(),
        Args::Monitor => monitor::run(),
        Args::Watch => watcher::run(),
        _ => client::client(&args),
    };

    match res {
        Ok(()) => (),
        Err(err) => {
            eprintln!("{err:?}");
            process::exit(1);
        }
    }
}
