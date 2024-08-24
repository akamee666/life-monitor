use keylogger::KeyLogger;
use win::{process::ProcessTracker, systray};

mod db;
mod keylogger;
mod logger;
mod shutdown;
mod win;

#[tokio::main]
#[cfg(target_os = "windows")]
async fn main() {
    logger::init_logger();

    tokio::spawn(systray::init());
    tokio::spawn(ProcessTracker::track_processes());
    crate::KeyLogger::start_logging().await;
}
