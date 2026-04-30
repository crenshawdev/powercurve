//! Example: drive `FanDaemon` directly in a 1Hz loop without the daemon
//! binary or D-Bus, as a smoke test for the curve evaluator.

use log::LevelFilter;
use powercurve::{
    fan::{FanDaemon, FanDaemonError},
    logging,
    nvml::NvidiaState,
};
use std::{process, thread, time};

fn inner() -> Result<(), FanDaemonError> {
    let mut daemon = FanDaemon::new(NvidiaState::Absent);

    loop {
        daemon.step();
        thread::sleep(time::Duration::new(1, 0));
    }
}

fn main() {
    if let Err(why) = logging::setup(LevelFilter::Debug) {
        eprintln!("failed to set up logging: {why}");
        process::exit(1);
    }

    if let Err(err) = inner() {
        eprintln!("{err:?}");
        process::exit(1);
    }
}
