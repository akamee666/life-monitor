use super::util::get_active_window;
use lazy_static::lazy_static;
use std::sync::{Mutex, MutexGuard};
use std::thread::sleep;
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

    // war against borrow checker below, probably there are some war crimes but it's not hurting
    // performance too much
    pub async fn track_processes() {
        let mut mutex_changer = TRACKER.lock().unwrap();

        let mut i = 0;
        let mut idle = false;
        loop {
            i = i + 1;

            // wait one second each time through
            sleep(Duration::from_secs(1));
            // every five seconds we check if the last input time is greater than 30 seconds, if it's
            // we pause tracking cause user is probably idle.
            if i == mutex_changer.idle_check {
                let duration = super::util::get_last_input_time().as_secs();
                if mutex_changer.idle_period > 0 && duration > mutex_changer.idle_period {
                    idle = true;
                } else {
                    idle = false;
                }
                i = 0;
            }
            if !idle {
                Self::get_process(&mut mutex_changer)
            }
        }
    }

    fn get_process(mutex_changer: &mut MutexGuard<ProcessTracker>) {
        let window_pid = get_active_window();

        if window_pid == 0 {
            return;
        }

        // it's probably a skill issue do this in that way but i dont care fuck off
        let sys = sysinfo::System::new_all();
        let process = sys.process(Pid::from_u32(window_pid)).unwrap();
        let process_name = process.name();

        println!("Active window[{}] title: {}", window_pid, process_name);

        // it means that we are in the first call or in different window than before.
        if mutex_changer.time == 0 {
            mutex_changer.time = System::uptime();
            mutex_changer.last_window_name = process_name.to_string();
        }

        if mutex_changer.last_window_name != process_name {
            let time_spent_diff = System::uptime() - mutex_changer.time;
            mutex_changer.time = 0;
            let app_name = mutex_changer.last_window_name.clone();
            Self::update_time_for_app(app_name.as_str(), time_spent_diff, mutex_changer);
        }
    }

    fn update_time_for_app(
        app_name: &str,
        time: u64,
        mutex_changer: &mut MutexGuard<ProcessTracker>,
    ) {
        if mutex_changer.tracking_data.contains_key(app_name) {
            let mut time_from_app = *mutex_changer.tracking_data.get(app_name).unwrap();
            time_from_app = time_from_app + time;
            mutex_changer
                .tracking_data
                .insert(app_name.to_string(), time_from_app);
            println!("We already have this app in our hashmap, increasing time...");
        } else {
            println!("We dont have this app yet, inserting new value...");
            mutex_changer
                .tracking_data
                .insert(app_name.to_string(), time);
        }

        for (name, time) in mutex_changer.tracking_data.iter() {
            println!("name:{name}, time spent: {time}");
        }
    }
}
