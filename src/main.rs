//#![windows_subsystem = "windows"]

mod db;
mod keylogger;
mod logger;
mod processinfo;

#[cfg(target_os = "windows")]
mod win;

#[cfg(target_os = "windows")]
#[tokio::main]
async fn main() {
    // TODO:
    // 1# Ask for arguments to where save db file.
    // 2# Print a message and flags to remove features.
    // 3# Fix mouse accuracy.
    // 4# Change data struct when getting processes to handle name of the window as well.

    logger::init_logger();
    tokio::spawn(win::systray::init());
    tokio::spawn(win::process::ProcessTracker::track_processes());

    keylogger::KeyLogger::start_logging().await;
}

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() {
    logger::init_logger();
    tokio::spawn(linux::process::ProcessTracker::track_processes());
    keylogger::KeyLogger::start_logging().await;
}
