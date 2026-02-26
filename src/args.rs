// Copyright 2018-2022 System76 <info@system76.com>
//
// SPDX-License-Identifier: GPL-3.0-only

use clap::{builder::PossibleValuesParser, Parser};

#[derive(Parser)]
#[clap(
    name = "powercurve",
    about = "Desktop power management daemon",
    version = env!("CARGO_PKG_VERSION"),
    subcommand_required = true,
    arg_required_else_help = true,
)]
pub enum Args {
    #[clap(
        about = "Runs the program in daemon mode",
        long_about = "Registers a new DBUS service and starts an event loop to listen for, and \
                      respond to, DBUS events from clients"
    )]
    Daemon {
        #[clap(
            short = 'q',
            long = "quiet",
            help = "Set the verbosity of daemon logs to 'off' [default is 'info']",
            global = true,
            group = "verbosity"
        )]
        quiet:   bool,
        #[clap(
            short = 'v',
            long = "verbose",
            help = "Set the verbosity of daemon logs to 'debug' [default is 'info']",
            global = true,
            group = "verbosity"
        )]
        verbose: bool,
    },
    #[clap(
        about = "Query or set the power profile",
        long_about = "Queries or sets the power profile.\n\n - If an argument is not provided, \
                      the power profile will be queried\n - Otherwise, that profile will be set, \
                      if it is a valid profile"
    )]
    Profile {
        #[clap(
            help = "set the power profile",
            default_value = None,
            value_parser = PossibleValuesParser::new(["quiet", "balanced", "performance"]),
        )]
        profile: Option<String>,
    },
    #[clap(
        name = "fan-detect",
        about = "Detect hwmon devices and generate a starter fan.toml"
    )]
    FanDetect {
        #[clap(
            long = "generate",
            help = "Output only the generated fan.toml config (no device summary)"
        )]
        generate: bool,
    },
    #[clap(
        name = "config",
        about = "Validate the fan configuration file",
        long_about = "Loads and validates /etc/powercurve/fan.toml, reporting any errors or \
                      warnings. Does not require root"
    )]
    Config,
    #[clap(
        about = "Show current daemon status",
        long_about = "Displays the current power profile, temperatures, and fan duties. \
                      Connects to the running daemon via D-Bus. Does not require root"
    )]
    Status,
}
