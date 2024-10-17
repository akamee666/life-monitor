use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio::time::Duration;

use crate::data::DataStore;
use crate::data::StorageBackend;

use tracing::*;

#[cfg(target_os = "linux")]
use crate::linux::util::get_idle_time;

#[cfg(target_os = "windows")]
use crate::win::util::get_idle_time;

pub mod api;
pub mod args;
pub mod data;
pub mod keylogger;
pub mod localdb;
pub mod logger;
pub mod processinfo;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod win;

#[derive(Debug, Clone, Copy)]
pub enum Event {
    Tick,
    IdleCheck,
    DbUpdate,
}

#[derive(Debug)]
pub struct ProcessTracker {
    time: u64,
    last_window_class: String,
    idle_period: u64,
    procs: Vec<ProcessInfo>,
}

impl ProcessTracker {
    async fn new(backend: &StorageBackend) -> Self {
        let d: Vec<ProcessInfo> = backend.get_proc_data().await.unwrap_or_else(|err| {
            error!("Call to backend to get keys data failed, quitting!\nError: {err}",);
            panic!();
        });

        ProcessTracker {
            time: 0,
            last_window_class: String::new(),
            idle_period: 20,
            procs: d,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessInfo {
    pub window_name: String,
    pub window_time: u64,
    pub window_instance: String,
    pub window_class: String,
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

pub fn check_idle(idle_period: &u64) -> bool {
    debug!("Checking if user is idle");
    let duration = get_idle_time().unwrap().as_secs();
    if duration > *idle_period {
        debug!("User is idle, stopping!");
        true
    } else {
        debug!("User is not idle.");
        false
    }
}

// FIX:
// Something is fucked up here.
fn new_window(
    tracking_data: &mut [ProcessInfo],
    window_name: &str,
    window_class: &str,
    window_instance: &str,
    time: u64,
) -> bool {
    debug!("Checking new window in Vec");
    if let Some(info) = tracking_data
        .iter_mut()
        .find(|p| p.window_instance == window_instance && p.window_class == window_class)
    {
        info.window_time += time;
        if info.window_name != window_name {
            debug!(
                "Different name when updating window, info.name: {}. window_name: {}",
                info.window_name, window_name
            );
            info.window_name = window_name.to_string();
        }
        false
    } else {
        true
    }
}

pub fn update_window_time(
    tracking_data: &mut Vec<ProcessInfo>,
    window_name: String,
    window_class: String,
    window_instance: String,
    time_diff: u64,
) {
    if new_window(
        tracking_data,
        &window_name,
        &window_class,
        &window_instance,
        time_diff,
    ) {
        debug!("Adding new entry in Vector");
        // If it's new, add a new entry.
        tracking_data.push(ProcessInfo {
            window_name,
            window_time: time_diff,
            window_instance,
            window_class,
        });
    }
}
