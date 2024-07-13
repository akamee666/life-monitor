// The main purpose of this project is to create a program that will monitor the amount of times
// that i use the outputs of my setup, my keyboard, mouse and also monitor what i'm doing daily.
// The whole point of this is to create some graphs in an personal blog as i explained in README.md
use rdev::{listen, Event};
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
    // each time an event occurs, increment the global var by one.
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
            // https://stackoverflow.com/questions/68288183/detect-mousemove-of-x-pixels
            if LAST_X_PX != 0.0 {
                let power_x: f64 = (LAST_Y_PX - y).powf(2.0);
                let power_y: f64 = (LAST_X_PX - x).powf(2.0);

                MOUSE_MOV += (power_x + power_y).sqrt();
            }

            LAST_X_PX = x;
            LAST_Y_PX = y;
        },

        _ => {}
    }
}

static mut TIME_CODING: u64 = 0;
fn find_running_processes() {
    let mut time_alacritty: u64 = 0;
    loop {
        thread::sleep(Duration::from_millis(100));
        let mut sys = System::new_all();

        // Update information of your struct
        sys.refresh_all();

        // The logic here is valid but only if i have one opened window at the moment.
        for (pid, process) in sys.processes() {
            if process.status() == ProcessStatus::Run && process.name() != "life-agent-for-" {
                match process.name() {
                    // coding.
                    "nvim" => unsafe {
                        // Solution: ? # Changing the library to have a function like set_run_time
                        // would work in this case, i think.
                        //
                        // System -> inner: SystemInner -> processes: HashMap<Pid,Process> , global_cpu: Cpu
                        // The set_run_time function should be in process.rs -> Impl ProcessInner
                        //
                        // 1. First time opening the program.
                        // 2. Another windows is already running.
                        // 3. The program had been reopened and the timer go back to zero.
                        // ### UNRELATED
                        // common.rs from sysinfo library can help you with a idea to increment the
                        // right kind of time
                        if process.run_time() < TIME_CODING {
                            TIME_CODING += process.run_time();

                            println!("increment the runtime");
                            println!(
                                "name: [{:?}], pid: [{:?}], time_coding: {:?}s, current_time: {:?}s",
                                process.name(),
                                process.pid(),
                                TIME_CODING,
                                process.run_time()
                            );

                            break;
                        } else {
                            TIME_CODING = process.run_time();
                            println!("get the run_time");
                            println!(
                                "name: [{:?}], pid: [{:?}], time_coding: {:?}s, current_time: {:?}s",
                                process.name(),
                                process.pid(),
                                TIME_CODING,
                                process.run_time()
                            );
                        }
                    },

                    // just to clippy dont mess with my code.
                    "123" => unsafe {
                        // if this is true than the program was closed and reopened.
                        if process.run_time() < time_alacritty {
                            time_alacritty = process.run_time();
                            TIME_CODING += process.run_time() + time_alacritty;

                            println!(
                             "name: [{:?}] pid: {:?} TIME_CODING: {:?}, time_alacritty: {:?}, start: {:?}",
                             process.name(),
                             process.pid(),
                             TIME_CODING,
                             time_alacritty,
                             process.start_time(),
                         );
                            break;
                        }

                        TIME_CODING += process.run_time() - time_alacritty;
                        time_alacritty = process.run_time();

                        println!(
                             "name: [{:?}] pid: {:?} TIME_CODING: {:?}, time_alacritty: {:?}, start: {:?}",
                             process.name(),
                             process.pid(),
                             TIME_CODING,
                             time_alacritty,
                             process.start_time(),
                         );
                    },
                    _ => {}
                }
            }
        }
    }
}

fn main() {
    //https://rust-lang.github.io/async-book/01_getting_started/02_why_async.html
    find_running_processes();

    if let Err(error) = listen(callback) {
        println!("Error: {:?}", error)
    }
}
