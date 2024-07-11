// The main purpose of this project is to create a program that will monitor the amount of times
// that i use the outputs of my setup, my keyboard, mouse and also monitor what i'm doing daily.
// The whole point of this is to create some graphs in an personal blog as i explained in README.md
use chrono::{DateTime, Timelike, Utc};
use core::panic;
use rdev::{listen, Event};
use std::fs::*;
use std::io::*;
use std::thread;
use std::time::Duration;
use sysinfo::*;
static mut LEFT_CLICKS: i64 = 0;
static mut MIDDLE_CLICKS: i64 = 0;
static mut RIGHT_CLICKS: i64 = 0;
static mut KEY_PRESSED: i64 = 0;
static mut MOUSE_MOV: f64 = 0.0;
static mut LAST_X_PX: f64 = 0.0;
static mut LAST_Y_PX: f64 = 0.0;

fn callback(event: Event) {
    match event.event_type {
        rdev::EventType::ButtonPress(button) => match button {
            rdev::Button::Left => unsafe {
                LEFT_CLICKS += 1;
                println!("amount of left clicks: {}", LEFT_CLICKS);
            },
            rdev::Button::Right => unsafe {
                RIGHT_CLICKS += 1;
                println!("amount of right clicks: {}", RIGHT_CLICKS);
            },
            rdev::Button::Middle => unsafe {
                MIDDLE_CLICKS += 1;
                println!("amount of middle clicks: {}", MIDDLE_CLICKS);
            },
            _ => {}
        },

        rdev::EventType::KeyPress(_) => unsafe {
            KEY_PRESSED += 1;
            println!("amount of key pressed: {}", KEY_PRESSED);
        },

        rdev::EventType::MouseMove { x, y } => unsafe {
            if LAST_X_PX != 0.0 {
                let power_x: f64 = (LAST_Y_PX - y).powf(2.0);
                let power_y: f64 = (LAST_X_PX - x).powf(2.0);

                MOUSE_MOV += (power_x + power_y).sqrt();
            }

            LAST_X_PX = x;
            LAST_Y_PX = y;
            //println!("amount of mouse movement: {}cm", (MOUSE_MOV * 0.026) as i64);
        },

        _ => {}
    }
}

fn log(file: &mut File, s: String) {
    #[cfg(debug_assertions)]
    {
        print!("{}", s);
    }

    match file.write(s.as_bytes()) {
        Err(e) => {
            println!("Couldn't write to log file: {}", e)
        }
        _ => {}
    }

    match file.flush() {
        Err(e) => {
            println!("Couldn't flush log file: {}", e)
        }
        _ => {}
    }
}

fn create_log_file() -> File {
    let now: DateTime<Utc> = Utc::now();
    let filename = format!(
        "log-{}-{:02}+{:02}+{:02}.log",
        now.date_naive(),
        now.hour(),
        now.minute(),
        now.second()
    );

    let logfile = {
        match OpenOptions::new().write(true).create(true).open(&filename) {
            Ok(f) => f,

            Err(e) => {
                panic!("Could not create the log file {}", e)
            }
        }
    };
    logfile
}

fn find_running_processes() {
    // am i going to have categories for apps running?
    // gaming, chatting, procastination, coding :)
    loop {
        thread::sleep(Duration::from_millis(10));
        let mut sys = System::new_all();

        // Update information of your struct
        sys.refresh_all();

        // Display processes ID, name na disk usage:
        for (pid, process) in sys.processes() {
            if process.status() == ProcessStatus::Run && process.name() != "life-agent-for-" {
                println!(
                    "[{pid}] name: {:?} memory: {:?} run_time: {:?} status: {:?}",
                    process.name(),
                    process.memory(),
                    process.run_time(),
                    process.status()
                );
            }
        }
    }
}

fn main() {
    //let logfile_fd = create_log_file();
    find_running_processes();
    if let Err(error) = listen(callback) {
        println!("Error: {:?}", error)
    }
}

//
// fn log_header(file: &mut File) {
//     let os_info = {
//         let info = os_info::get();
//         format!(
//             "OS: type: {}\nVersion: {}\n",
//             info.os_type(),
//             info.version()
//         )
//     };
//
//     log(file, os_info);
//     let os_hostname = hostname::get().unwrap().into_string().unwrap();
//     println!("hostname: {:?}", os_hostname);
//     log(file, os_hostname);
// }
//
//fn run_spy(fd: &mut File) {

//GetUserDefaultLocaleName
//GetForegroundWindow
//GetwindowsthereadProcessId
//OpenProcess
//GetProcessImageFileNameW
//GetWindowTextLengthW
//GetWindowTextW
//GetAsyncKeyState
//}
