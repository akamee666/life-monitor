#![windows_subsystem = "windows"]

use keylogger::KeyLogger;
use win::{process::ProcessTracker, systray};

mod db;
mod keylogger;
mod logger;
mod shutdown;
mod win;

#[tokio::main]
async fn main() {
    // TODO:
    // 1# Add linux support again.
    // 2# Ask for arguments to setup database.
    // 3# Fix mouse accuracy.
    // 4# Change data struct when getting processes to handle name of the window as well.

    logger::init_logger();
    tokio::spawn(systray::init());
    tokio::spawn(ProcessTracker::track_processes());
    crate::KeyLogger::start_logging().await;
}
