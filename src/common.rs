//! This file is responsible to store functions, enums or
//! structs that can be used for all platforms supported.
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio::time::Duration;

use crate::storage::backend::*;

use tracing::*;

use std::env;
use std::io::{self};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub enum Event {
    Tick,
    IdleCheck,
    DbUpdate,
}

#[derive(Debug)]
pub struct ProcessTracker {
    pub time: u64,
    pub last_wname: String,
    pub last_wclass: String,
    pub idle_period: u64,
    pub procs: Vec<ProcessInfo>,
}

impl ProcessTracker {
    pub async fn new(backend: &StorageBackend) -> Self {
        let d: Vec<ProcessInfo> = backend.get_proc_data().await.unwrap_or_else(|err| {
            error!("Call to backend to get keys data failed, quitting!\nError: {err}",);
            panic!();
        });

        ProcessTracker {
            time: 0,
            last_wname: String::new(),
            last_wclass: String::new(),
            idle_period: 20,
            procs: d,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessInfo {
    pub w_name: String,
    pub w_time: u64,
    pub w_class: String,
}

pub fn spawn_ticker(tx: mpsc::Sender<Event>, duration: Duration, event: Event) {
    debug!("Spawning ticker: {:?}", event);
    tokio::spawn(async move {
        let mut interval = interval(duration);
        loop {
            interval.tick().await;
            if tx.send(event).await.is_err() {
                break;
            }
        }
    });
}

/// Returns a platform-specific path for storing program-related files and ensures the directory exists.
///
/// On Windows: `%LOCALAPPDATA%\akame_monitor`  
/// On Linux: `$HOME/.local/share/akame_monitor`  
///
/// # Errors
/// Returns an error if the required environment variable is not set, if the OS is unsupported,
/// or if the directory cannot be created.
pub fn program_data_dir() -> io::Result<PathBuf> {
    // Determine the base directory for application data
    let base_dir = if cfg!(target_os = "windows") {
        env::var("LOCALAPPDATA").map(PathBuf::from).map_err(|_| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "LOCALAPPDATA environment variable not set",
            )
        })?
    } else if cfg!(target_os = "linux") {
        let home = env::var("HOME").map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, "HOME environment variable not set")
        })?;
        let mut path = PathBuf::from(home);
        path.push(".local");
        path.push("share");
        path
    } else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unsupported operating system",
        ));
    };

    // Append application folder
    let path = base_dir.join("life_monitor");

    // Ensure the directory exists
    std::fs::create_dir_all(&path)?;

    Ok(path)
}
