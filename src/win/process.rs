use crate::{
    db::{get_process_data, send_to_process_table},
    processinfo::ProcessInfo,
    win::util::get_focused_window,
    win::util::get_last_input_time,
};
use once_cell::sync::Lazy;
use std::{sync::Mutex, time::Duration};
use sysinfo::System;
use tracing::*;

static TRACKER: Lazy<Mutex<ProcessTracker>> = Lazy::new(|| Mutex::new(ProcessTracker::new()));
#[derive(Debug)]
pub struct ProcessTracker {
    time: u64,
    last_window_name: String,
    idle_check: i32,
    idle_period: u64,
    tracking_data: Vec<ProcessInfo>,
}

impl ProcessTracker {
    pub fn new() -> ProcessTracker {
        let idle_check = 10;
        let idle_period = 20;
        let time = 0;
        let last_window_name = String::new();
        let tracking_data = get_process_data().expect("Could not receive data from database!\n");
        ProcessTracker {
            time,
            last_window_name,
            idle_check,
            idle_period,
            tracking_data,
        }
    }

    pub async fn track_processes() {
        debug!("Spawned ProcessTracker thread");

        let pause_to_send_data = 300;
        let mut i = 0;
        let mut idle = false;
        let mut changer = TRACKER.lock().expect("poisoned");
        let mut j = 0;

        loop {
            i += 1;
            j += 1;

            std::thread::sleep(Duration::from_secs(1));

            if i == changer.idle_check {
                let duration = get_last_input_time().as_secs();

                if duration > changer.idle_period {
                    idle = true;
                    debug!("Info is currently idle, we should stop tracking!");
                } else {
                    idle = false;
                }
                i = 0;
            }

            if j == pause_to_send_data {
                let result = send_to_process_table(&changer.tracking_data);
                match result {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Error sending data to time_wasted table. Error: {e:?}");
                    }
                }
                j = 0;
            }

            if !idle {
                let (name, class) =
                    get_focused_window().unwrap_or(("".to_string(), "".to_string()));

                debug!("name: {}, class: {}", name, class);
                if !class.is_empty() {
                    let uptime = System::uptime();
                    if !changer.last_window_name.is_empty() && changer.last_window_name != class {
                        let time_diff = uptime - changer.time;
                        changer.time = 0;
                        Self::update_time_for_app(
                            &mut changer.tracking_data,
                            name.as_str(),
                            time_diff,
                            &class,
                        );
                    }

                    if changer.time == 0 {
                        changer.time = uptime;
                        changer.last_window_name = class;
                    }
                } else {
                    error!("Could not get active window!");
                }
            }
        }
    }

    fn update_time_for_app(
        tracking_data: &mut Vec<ProcessInfo>,
        app_name: &str,
        time: u64,
        window_class: &str, // Add window_class as a parameter
    ) {
        let mut found = false;

        for info in tracking_data.iter_mut() {
            if info.name == app_name {
                info.time_spent += time;
                found = true;
                break;
            }
        }

        if !found {
            tracking_data.push(ProcessInfo {
                name: app_name.to_string(),
                time_spent: time,
                instance: window_class.to_string(), // Set instance to window_class
                window_class: window_class.to_string(),
            });
        }
    }
}
