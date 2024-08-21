use env_logger;
use keylogger::KeyLogger;
use log::*;

pub mod db;
mod keylogger;
mod shutdown;
mod win;

#[tokio::main]
async fn main() -> Result<(), windows_service::Error> {
    env_logger::init();
    info!("Starting program");

    // TODO: FIX DB ISSUES
    // TODO: I SUCK AT CODING NEED TO REFACTOR A LOT>
    // TODO: LOGGER in example seems good enough?
    tokio::spawn(crate::win::systray::init());
    tokio::spawn(crate::win::process::ProcessTracker::track_processes());
    KeyLogger::start_logging().await;

    // ok so seems to work that way.
    Ok(())
}
