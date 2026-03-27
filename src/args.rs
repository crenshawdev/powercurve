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
        quiet: bool,
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
    #[clap(name = "fan-detect", about = "Detect hwmon devices and generate a starter fan.toml")]
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
    #[clap(
        about = "Monitor profile and thermal events with desktop notifications",
        long_about = "Long-lived process that listens for PowerProfileSwitch and ThermalEvent \
                      signals from the daemon and sends desktop notifications. Does not require root"
    )]
    Monitor,
    #[clap(
        about = "Watch running processes and auto-switch power profiles",
        long_about = "Polls /proc for running processes and matches against rules defined in \
                      ~/.config/powercurve/watcher.toml. When a rule matches, the corresponding \
                      power profile is set via D-Bus. Does not require root"
    )]
    Watch,
    #[clap(about = "Print the version and exit")]
    Version,
    #[clap(
        about = "Temporarily override a fan channel's duty cycle",
        long_about = "Sets a temporary duty override on a PWM channel for testing.\n\n\
                      powercurve fan pwm3 50    - set pwm3 to 50%\n\
                      powercurve fan pwm3 clear - remove override, return to curve control\n\n\
                      Overrides last until the next profile change or until cleared"
    )]
    Fan {
        #[clap(help = "PWM channel name (e.g. pwm3)")]
        channel: String,
        #[clap(help = "Duty cycle percentage (0-100) or 'clear' to remove override")]
        duty: String,
    },
    #[clap(
        name = "fan-test",
        about = "Find the minimum duty where a fan spins",
        long_about = "Ramps duty from a starting percentage upward in fixed increments, reading\n\
                      RPM at each step, until the fan starts spinning. Reports the minimum duty\n\
                      as a suggested min_duty value for fan.toml.\n\n\
                      Requires the daemon to be running. Other fans keep normal curve control\n\
                      during the test. The override is cleared when the test finishes or on Ctrl-C"
    )]
    FanTest {
        #[clap(help = "PWM channel name (e.g. pwm1)")]
        channel: String,
        #[clap(long, default_value = "5", help = "Duty step increment (percent)")]
        step: u8,
        #[clap(long, default_value = "5", help = "Starting duty (percent)")]
        start: u8,
        #[clap(long, default_value = "2000", help = "Settle time per step (ms)")]
        settle: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_daemon() {
        let args = Args::parse_from(["powercurve", "daemon"]);
        assert!(matches!(args, Args::Daemon { quiet: false, verbose: false }));
    }

    #[test]
    fn parse_daemon_verbose() {
        let args = Args::parse_from(["powercurve", "daemon", "--verbose"]);
        assert!(matches!(args, Args::Daemon { verbose: true, .. }));
    }

    #[test]
    fn parse_profile_set() {
        let args = Args::parse_from(["powercurve", "profile", "balanced"]);
        match args {
            Args::Profile { profile } => assert_eq!(profile.as_deref(), Some("balanced")),
            _ => panic!("expected Profile variant"),
        }
    }

    #[test]
    fn parse_profile_query() {
        let args = Args::parse_from(["powercurve", "profile"]);
        match args {
            Args::Profile { profile } => assert!(profile.is_none()),
            _ => panic!("expected Profile variant"),
        }
    }

    #[test]
    fn parse_status() {
        let args = Args::parse_from(["powercurve", "status"]);
        assert!(matches!(args, Args::Status));
    }

    #[test]
    fn parse_config() {
        let args = Args::parse_from(["powercurve", "config"]);
        assert!(matches!(args, Args::Config));
    }

    #[test]
    fn parse_monitor() {
        let args = Args::parse_from(["powercurve", "monitor"]);
        assert!(matches!(args, Args::Monitor));
    }

    #[test]
    fn parse_fan_detect() {
        let args = Args::parse_from(["powercurve", "fan-detect"]);
        assert!(matches!(args, Args::FanDetect { generate: false }));
    }

    #[test]
    fn parse_fan_detect_generate() {
        let args = Args::parse_from(["powercurve", "fan-detect", "--generate"]);
        assert!(matches!(args, Args::FanDetect { generate: true }));
    }

    #[test]
    fn parse_watch() {
        let args = Args::parse_from(["powercurve", "watch"]);
        assert!(matches!(args, Args::Watch));
    }

    #[test]
    fn parse_version() {
        let args = Args::parse_from(["powercurve", "version"]);
        assert!(matches!(args, Args::Version));
    }

    #[test]
    fn parse_fan_override() {
        let args = Args::parse_from(["powercurve", "fan", "pwm3", "50"]);
        match args {
            Args::Fan { channel, duty } => {
                assert_eq!(channel, "pwm3");
                assert_eq!(duty, "50");
            }
            _ => panic!("expected Fan variant"),
        }
    }

    #[test]
    fn parse_fan_clear() {
        let args = Args::parse_from(["powercurve", "fan", "pwm3", "clear"]);
        match args {
            Args::Fan { channel, duty } => {
                assert_eq!(channel, "pwm3");
                assert_eq!(duty, "clear");
            }
            _ => panic!("expected Fan variant"),
        }
    }

    #[test]
    fn parse_fan_test() {
        let args = Args::parse_from(["powercurve", "fan-test", "pwm1"]);
        match args {
            Args::FanTest { channel, step, start, settle } => {
                assert_eq!(channel, "pwm1");
                assert_eq!(step, 5);
                assert_eq!(start, 5);
                assert_eq!(settle, 2000);
            }
            _ => panic!("expected FanTest variant"),
        }
    }

    #[test]
    fn parse_fan_test_options() {
        let args = Args::parse_from([
            "powercurve",
            "fan-test",
            "pwm3",
            "--step",
            "3",
            "--start",
            "10",
            "--settle",
            "3000",
        ]);
        match args {
            Args::FanTest { channel, step, start, settle } => {
                assert_eq!(channel, "pwm3");
                assert_eq!(step, 3);
                assert_eq!(start, 10);
                assert_eq!(settle, 3000);
            }
            _ => panic!("expected FanTest variant"),
        }
    }

    #[test]
    fn invalid_profile_rejected() {
        let result = Args::try_parse_from(["powercurve", "profile", "turbo"]);
        assert!(result.is_err());
    }
}
