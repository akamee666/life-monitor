use super::util::get_active_window;
use lazy_static::lazy_static;
use std::sync::{Mutex, MutexGuard};
use std::thread::sleep;
use std::{collections::HashMap, time::Duration};
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
    // // performance too much
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
                let duration = user_idle::UserIdle::get_time().unwrap().as_seconds();
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
        let window_class = get_active_window().unwrap_or(String::new());
        // i probably can optimize it a little better.

        if !window_class.is_empty() {
            let window_class_lower = window_class.to_lowercase();

            for process in mutex_changer.sys.processes_by_name(&window_class_lower) {
                println!(
                    "pid: [{}] Active Window: [{}]",
                    process.pid(),
                    process.name(),
                );
                break;
            }

            // it means that we are in the first call or in different window than before.
            if mutex_changer.time == 0 {
                mutex_changer.time = System::uptime();
                mutex_changer.last_window_name = window_class_lower.clone();
            }

            if mutex_changer.last_window_name != window_class_lower {
                let time_spent_diff = System::uptime() - mutex_changer.time;
                mutex_changer.time = 0;
                let app_name = mutex_changer.last_window_name.clone();
                Self::update_time_for_app(app_name.as_str(), time_spent_diff, mutex_changer);
            }
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

/*  So i can keep it simple and just have like the program name and time or if i want to i can complicate stuff
*   and also get the name of what's running inside that program like get the tab name in firefox
*   and stuff like that.
*
*   In this case we have an HashMap<K,V> where K is the program name and V is the time.
*   Simple:
*   Program: Alacritty,
*   time: 99,
*
*   In this other case i dont even now what i would have.
*   Complicate:
*   Program: Alacritty,
*   name: nvim $some_file,
*   time: 99,
*
*   The complicate case would give me more specific graphs like which site i'm using more or which program is running on my terminal and but it would be a lot more complicate to
*   build as well.
*
*   The simple case is almost done already but it would give me generics graphs like, "alacritty"
*   55% of my time, firefox(googling aroung) 45% of my time and so on.
*
*   Also i have that felling that this code COULD have a better performance avoiding some
*   redundancies. For what i have tested so far it's using max of 5% of my cpu(AMD 5600) in linux whenever i
*   change focus between windows, don't seems bad by now but at some point i would have to upload
*   this data to a local database and push it to a api, that will definitely uses more than 5% of a
*   mid-end cpu, is it acceptable in this case? I dont know.
*/
