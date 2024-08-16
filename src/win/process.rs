use super::util::get_active_window;
use crate::win::util::*;
use lazy_static::lazy_static;
use std::sync::Mutex;
use std::{collections::HashMap, time::Duration};
use sysinfo::Pid;
use sysinfo::System;

lazy_static! {
    static ref TRACKER: Mutex<ProcessTracker> = {
        let tracker = Mutex::new(ProcessTracker::new());
        tracker
    };
}

#[derive(Debug)]
pub struct ProcessTracker {
    time: u64,
    last_window_name: String,
    idle_check: i32,
    idle_period: u64,
    sys: System,
    tracking_data: HashMap<String, u64>,
}

impl ProcessTracker {
    pub fn new() -> ProcessTracker {
        let sys = sysinfo::System::new_all();
        let idle_check = 5;
        let idle_period = 20;
        let time = 0;
        let last_window_name = String::new();
        let tracking_data = HashMap::new();
        ProcessTracker {
            time,
            last_window_name,
            idle_check,
            idle_period,
            sys,
            tracking_data,
        }
    }

    pub async fn track_processes() {
        println!("starting tracking");

        let mut i = 0;
        let mut idle = false;

        loop {
            i = i + 1;

            std::thread::sleep(Duration::from_secs(1));

            let mut changer = TRACKER.lock().expect("poisoned");
            // every five seconds we check if the last input time is greater than 30 seconds, if it's
            // we pause tracking cause user is probably idle.
            if i == changer.idle_check {
                let duration = get_last_input_time().as_secs();

                // need to sent data here.
                println!("should be calling get db now");

                if changer.idle_period > 0 && duration > changer.idle_period {
                    idle = true;
                } else {
                    idle = false;
                }
                i = 0;
            }
            if !idle {
                let proc_name = Self::get_process(&changer.sys);
                if !proc_name.is_empty() {
                    let uptime = System::uptime();

                    if !changer.last_window_name.is_empty() && changer.last_window_name != proc_name
                    {
                        let app_name: String = changer.last_window_name.clone();
                        let time_diff = uptime - changer.time;
                        changer.time = 0;
                        Self::update_time_for_app(
                            &mut changer.tracking_data,
                            app_name.as_str(),
                            time_diff,
                        );
                    }

                    // it means that we are in the first call or in different window than before.
                    if changer.time == 0 {
                        changer.time = uptime;
                        changer.last_window_name = proc_name.clone();
                    }
                }
            }
        }
    }

    // TODO: I NEED A FIX
    fn get_process(sys: &System) -> String {
        let (window_pid, title) = get_active_window();

        if window_pid == 0 {
            return "".to_string();
        }

        let process = sys.processes().get(&Pid::from_u32(window_pid));
        if let Some(process) = process {
            let process_name = process.name();
            println!("Active window[{}] title: {}", window_pid, title);

            return process_name.to_string();
        } else {
            return "".to_string();
        }
    }

    fn update_time_for_app(tracking_data: &mut HashMap<String, u64>, app_name: &str, time: u64) {
        if tracking_data.contains_key(app_name) {
            let time_from_app = tracking_data.get(app_name).unwrap();
            let time_diff_to_add = time_from_app + time;
            tracking_data.insert(app_name.to_string(), time_diff_to_add);
            println!("We already have this app in our hashmap, increasing time...");
        } else {
            println!("We dont have this app yet, inserting new value...");
            tracking_data.insert(app_name.to_string(), time);
        }

        for (name, time) in tracking_data.iter() {
            println!("name:{name}, time spent: {time}");
        }
    }
}
