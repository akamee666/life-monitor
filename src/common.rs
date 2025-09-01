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

// Is this function really necessary?.
pub fn find_path() -> Result<PathBuf, std::io::Error> {
    let path = if cfg!(target_os = "windows") {
        let local_app_data = env::var("LOCALAPPDATA").map_err(|_| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "LOCALAPPDATA environment variable not set",
            )
        })?;
        let mut path = PathBuf::from(local_app_data);
        path.push("akame_monitor");

        path
    } else if cfg!(target_os = "linux") {
        let home_dir = env::var("HOME").map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, "HOME environment variable not set")
        })?;
        let mut path = PathBuf::from(home_dir);
        path.push(".local");
        path.push("share");
        path.push("akame_monitor");

        path
    } else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unsupported operating system",
        ));
    };

    Ok(path)
}
