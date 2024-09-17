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
    // https://www.reddit.com/r/rust/comments/f7yrle/comment/hsgz16m/?utm_source=share&utm_medium=web3x&utm_name=web3xcss&utm_term=1&utm_content=share_button
    // 2# Ask for arguments to setup database.
    // 3# Fix mouse accuracy.
    // 4# Change data struct when getting processes to handle name of the window as well.

    logger::init_logger();
    tokio::spawn(systray::init());
    tokio::spawn(ProcessTracker::track_processes());
    crate::KeyLogger::start_logging().await;
}
