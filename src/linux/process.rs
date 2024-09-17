use crate::{
    db::*,
    linux::util::{get_focused_window, get_idle_time},
    processinfo::ProcessInfo,
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
        let tracking_data = get_process_data().expect("Could not receive data from database!");

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
        let mut j = 0;
        let mut idle = false;

        // Lock tracker for thread safety
        let mut tracker = TRACKER.lock().expect("poisoned");

        //  So basically we check everytime if in the last ten seconds any input were received.
        //  If after twenty seconds any inputs were received, user is probably idle so we pause
        //  tracking and send data to database cause nothing is being received so it's just
        //  wasteful if we keep sending the same data over and over again.
        loop {
            i += 1;
            j += 1;

            std::thread::sleep(Duration::from_secs(1));

            if i == tracker.idle_check {
                let duration = get_idle_time().unwrap().as_secs();

                if duration > tracker.idle_period {
                    idle = true;
                    debug!("Info is currently idle, we should stop tracking!");
                } else {
                    idle = false;
                }
                i = 0;
            }

            if j == pause_to_send_data {
                let result = send_to_process_table(&tracker.tracking_data);
                if let Err(e) = result {
                    error!("Error sending data to time_wasted table. Error: {e:?}");
                }
                j = 0;
            }

            if !idle {
                let (name, instance, class) = get_focused_window().unwrap_or_default();
                debug!("name: {}, instance: {}, class: {}.", name, instance, class);

                if !class.is_empty() {
                    let uptime = System::uptime();

                    if !tracker.last_window_name.is_empty() && tracker.last_window_name != class {
                        let time_diff = uptime - tracker.time;
                        tracker.time = 0;

                        // Update process info
                        Self::update_time_for_app(
                            &mut tracker.tracking_data,
                            &name,
                            time_diff,
                            instance,
                            &class,
                        );
                    }

                    if tracker.time == 0 {
                        tracker.time = uptime;
                        tracker.last_window_name = class;
                    }
                }
            }
        }
    }

    fn update_time_for_app(
        tracking_data: &mut Vec<ProcessInfo>,
        app_name: &str,
        time: u64,
        instance: String,
        window_class: &str,
    ) {
        if let Some(info) = tracking_data.iter_mut().find(|p| p.name == app_name) {
            info.time_spent += time;
            info.instance = instance;
            info.window_class = window_class.to_string();
        } else {
            tracking_data.push(ProcessInfo {
                name: app_name.to_string(),
                time_spent: time,
                instance,
                window_class: window_class.to_string(),
            });
        }
    }
}
