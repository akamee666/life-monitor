use crate::{
    db::{get_process_data, send_to_process_table},
    win::util::get_active_window,
    win::util::get_last_input_time,
};
use once_cell::sync::Lazy;
use std::{collections::HashMap, sync::Mutex, time::Duration};
use sysinfo::{Pid, System};
use tracing::{debug, error};

static TRACKER: Lazy<Mutex<ProcessTracker>> = Lazy::new(|| Mutex::new(ProcessTracker::new()));

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
        let idle_check = 10;
        let idle_period = 20;
        let time = 0;
        let last_window_name = String::new();
        let tracking_data =
            get_process_data().expect("something failed getting the processes data from db");

        /* Return the values */
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
        debug!("Spawned ProcessTracker thread");

        let pause_to_send_data = 300;
        let mut i = 0;
        let mut idle = false;
        let mut changer = TRACKER.lock().expect("poisoned");
        let mut j = 0;

        //  So basically we check everytime if in the last ten seconds any input were received.
        //  If after twenty seconds any inputs were received, user is probably idle so we pause
        //  tracking and send data to database cause nothing is being received so it's just
        //  wasteful if we keep sending the same data over and over again.
        //
        loop {
            i = i + 1;
            j = j + 1;

            std::thread::sleep(Duration::from_secs(1));

            if i == changer.idle_check {
                let duration = get_last_input_time().as_secs();

                if duration > changer.idle_period {
                    idle = true;
                    debug!("Info is currently idle, we should stoping tracking!");
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

    fn get_process(sys: &System) -> String {
        let (window_pid, title) = get_active_window();

        if window_pid == 0 {
            return "".to_string();
        }

        let process = sys.processes().get(&Pid::from_u32(window_pid));
        if let Some(process) = process {
            let process_name = process.name();
            debug!("Active window[{}] title: {}", window_pid, title);

            return process_name.to_str().unwrap().to_string();
        } else {
            return "".to_string();
        }
    }

    fn update_time_for_app(tracking_data: &mut HashMap<String, u64>, app_name: &str, time: u64) {
        if tracking_data.contains_key(app_name) {
            let time_from_app = tracking_data.get(app_name).unwrap();
            let time_diff_to_add = time_from_app + time;
            tracking_data.insert(app_name.to_string(), time_diff_to_add);
        } else {
            tracking_data.insert(app_name.to_string(), time);
        }
    }
}
