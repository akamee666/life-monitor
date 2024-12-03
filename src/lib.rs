//! This file is responsible to deal with import of modules and define functions, enums or
//! structs that can be used for both platform. Maybe i should not do this that way because
//! sometimes i need to define everything as public and it might be bad? I dont know if i care though,
//! dont seem to have any downsides.

use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio::time::Duration;

use crate::backend::*;

use tracing::*;

#[cfg(target_os = "linux")]
use platform::linux::util::get_idle_time;

#[cfg(target_os = "windows")]
use platform::win::util::get_idle_time;

pub mod args;
pub mod keylogger;
pub mod logger;
pub mod platform;

pub mod backend;
mod storage;

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
    pub w_name: String,
    pub w_time: u64,
    pub w_instance: String,
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

// This function uses get_idle_time which is imported from win module or linux mod.
// I wonder if it's better to just one function and use cfg!(target_os = "x") instead of two
// different functions, each of them inside of its own platform module.
pub fn is_idle(idle_period: &u64) -> bool {
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

fn is_window_new(tracking_data: &mut [ProcessInfo], w_name: &str, time: u64) -> bool {
    // Tries to find if we already have the same w_name in the vector.
    if let Some(info) = tracking_data.iter_mut().find(|p| p.w_name == w_name) {
        debug!("Updating time for existent vector rather than adding new entry.");
        info.w_time += time;
        //debug!("{:#?}\n", tracking_data);
        false
    } else {
        debug!("Adding new entry to vector rather than updating its time.");
        true
    }
}

pub fn is_startup_enable() -> Result<bool, String> {
    use std::process;
    #[cfg(target_os = "linux")]
    {
        match platform::linux::util::check_startup_status() {
            Ok(status) => Ok(status),
            Err(e) => {
                error!("Failed to check startup status on Linux due: {}", e);
                process::exit(1);
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        match platform::win::util::check_startup_status() {
            Ok(status) => Ok(status), // Return the status if the call is successful
            Err(e) => {
                error!("Failed to check startup status on Windows due: {}", e); // Log the error
                process::exit(1); // Exit the program with error code 1
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        error!("Startup check not implemented for this operating system");
        process::exit(1); // Exit the program with error code 1
    }
}

pub fn update_window_time(
    tracking_data: &mut Vec<ProcessInfo>,
    w_name: String,
    w_class: String,
    w_instance: String,
    time_diff: u64,
) {
    if is_window_new(tracking_data, &w_name, time_diff) {
        tracking_data.push(ProcessInfo {
            w_name,
            w_time: time_diff,
            w_instance,
            w_class,
        });
        //debug!("After push: {:#?}", tracking_data);
    }
}
