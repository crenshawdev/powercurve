use log::LevelFilter;
use std::{process, thread, time};
use system76_power::{
    fan::{FanDaemon, FanDaemonError},
    logging,
};

fn inner() -> Result<(), FanDaemonError> {
    let mut daemon = FanDaemon::new(false);

    loop {
        daemon.step();
        thread::sleep(time::Duration::new(1, 0));
    }
}

fn main() {
    if let Err(why) = logging::setup(LevelFilter::Debug) {
        eprintln!("failed to set up logging: {}", why);
        process::exit(1);
    }

    if let Err(err) = inner() {
        eprintln!("{:?}", err);
        process::exit(1);
    }
}
