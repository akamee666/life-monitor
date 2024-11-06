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
    last_wname: String,
    last_wclass: String,
    last_winstance: String,
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
            last_wname: String::new(),
            last_wclass: String::new(),
            last_winstance: String::new(),
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

fn new_window(tracking_data: &mut [ProcessInfo], window_name: &str, time: u64) -> bool {
    // Tries to find if we already have the same window_name in the vector.
    if let Some(info) = tracking_data
        .iter_mut()
        .find(|p| p.window_name == window_name)
    {
        debug!("Updating time rather than adding new entry.");
        info.window_time += time;
        debug!("{:#?}\n", tracking_data);
        false
    } else {
        debug!("Adding new entry rather than updating time.");
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
    if new_window(tracking_data, &window_name, time_diff) {
        tracking_data.push(ProcessInfo {
            window_name,
            window_time: time_diff,
            window_instance,
            window_class,
        });
        debug!("After push: {:#?}", tracking_data);
    }
}
