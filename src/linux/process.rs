use super::util::get_active_window;
use lazy_static::lazy_static;
use std::sync::Mutex;
use std::{collections::HashMap, time::Duration};
use sysinfo::System;
use tokio::time;
// looks like static mut is bad for some reason that i dont really understand that deep, somewhere
// in future i will try to remove these.
const IDLE_CHECK_SECS: i32 = 5;
const IDLE_PERIOD: u64 = 20;
static mut TIME: u64 = 0;
static mut LAST_ACTIVE_WINDOW_NAME: String = String::new();

lazy_static! {
    static ref TRACKING_DATA_MAP: Mutex<HashMap<String, u64>> = {
        let tracking_data = Mutex::new(HashMap::new());
        tracking_data
    };
}

pub async fn track_processes() {
    let mut interval = time::interval(Duration::from_secs(1));

    let mut i = 0;
    let mut idle = false;
    loop {
        i = i + 1;

        // wait one second each time through
        interval.tick().await;

        // every five seconds we check if the last input time is greater than 30 seconds, if it's
        // we pause tracking cause user is probably idle.
        if i == IDLE_CHECK_SECS {
            let duration = user_idle::UserIdle::get_time().unwrap().as_seconds();
            if IDLE_PERIOD > 0 && duration > IDLE_PERIOD {
                idle = true;
            } else {
                idle = false;
            }
            i = 0;
        }
        if !idle {
            unsafe {
                get_process().await;
            }
        }
    }
}

fn update_time_for_app(app_name: &str, time: u64) {
    let mut mutex_changer = TRACKING_DATA_MAP.lock().unwrap();

    if mutex_changer.contains_key(app_name) {
        let mut time_from_app = *mutex_changer.get(app_name).unwrap();
        time_from_app = time_from_app + time;
        mutex_changer.insert(app_name.to_string(), time_from_app);
        println!("We already have this app in our hashmap, increasing time...");
    } else {
        println!("We dont have this app yet, inserting new value...");
        mutex_changer.insert(app_name.to_string(), time);
    }

    for (name, time) in mutex_changer.iter() {
        println!("name:{name}, time spent: {time}");
    }
}

async unsafe fn get_process() {
    let sys = sysinfo::System::new_all();
    let window_class = get_active_window().unwrap_or(String::new());
    // i probably can optimize it a little better.
    if !window_class.is_empty() {
        let window_class_lower = window_class.to_lowercase();

        for process in sys.processes_by_name(&window_class_lower) {
            println!(
                "pid: [{}] Active Window: [{}]",
                process.pid(),
                process.name(),
            );

            break;
        }

        // it means that we are in the first call or in different window than before.
        if TIME == 0 {
            TIME = System::uptime();
            LAST_ACTIVE_WINDOW_NAME = window_class_lower.clone();
        }

        if LAST_ACTIVE_WINDOW_NAME != window_class_lower {
            let time_spent_diff = System::uptime() - TIME;
            TIME = 0;
            update_time_for_app(LAST_ACTIVE_WINDOW_NAME.as_str(), time_spent_diff)
        }
    }
}
