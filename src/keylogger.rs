use log::debug;
use log::error;
use rdev::listen;
use std::ptr::addr_of;
use std::thread;
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio::time::Duration;

use crate::db::update_input_table;

// even if i read this data at the same time other thread is writing to it i dont think
// there will be a problem, unless there are weird memory bugs. it is being updated at each period
// of time so maybe it's fine leave it that way.
static mut EVENTS_COUNTER: KeyLogger = KeyLogger::new();
static mut LAST_X_PX: f64 = 0.0;
static mut LAST_Y_PX: f64 = 0.0;

#[allow(dead_code)]
#[derive(Debug)]
pub struct KeyLogger {
    pub left_clicks: u64,
    pub right_clicks: u64,
    pub middle_clicks: u64,
    pub keys_pressed: u64,
    pub pixels_moved: f64,
    pub mouse_moved_cm: u64,
}

impl KeyLogger {
    const fn new() -> KeyLogger {
        let left_clicks = 0;
        let right_clicks = 0;
        let middle_clicks = 0;
        let keys_pressed = 0;
        let mouse_moved_cm = 0;
        let pixels_moved = 0.0;

        /* Return the values */
        KeyLogger {
            left_clicks,
            right_clicks,
            middle_clicks,
            keys_pressed,
            pixels_moved,
            mouse_moved_cm,
        }
    }

    pub async fn start_logging() {
        debug!("Spawned KeyLogger thread");
        // thanks rdev devs for have left this example.
        // tx = transmiter.
        // rx = receiver.
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _listener = thread::spawn(move || {
            listen(move |event| {
                tx.send(event)
                    .unwrap_or_else(|e| error!("Could not send event {:?}", e));
            })
            .expect("Could not listen");
        });

        tokio::spawn(async {
            let mut interval = interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                unsafe {
                    // Convert pixels moved to cm before sending data to DB
                    // it's not accurate for all devices for sure, the
                    // division with 3.0 seems accurate for my mouse but if i change for another
                    // one it just fuck up with everything but i couldn't find a way to make it
                    // works so i'll leave it that way for now

                    EVENTS_COUNTER.mouse_moved_cm =
                        ((EVENTS_COUNTER.pixels_moved * 0.026) / 3.0) as u64;
                }

                unsafe {
                    let result = update_input_table(addr_of!(EVENTS_COUNTER));
                    match result {
                        Ok(_) => {}
                        Err(e) => {
                            // log somewhere in a file i think
                            error!("Error sending data to input table. Error: {e:?}");
                        }
                    }
                }

                unsafe {
                    EVENTS_COUNTER.print_counters();
                }
            }
        });

        loop {
            let event = rx.recv().await.expect("could not receive the event?");
            match event.event_type {
                rdev::EventType::ButtonPress(button) => match button {
                    rdev::Button::Left => unsafe { EVENTS_COUNTER.left_clicks += 1 },
                    rdev::Button::Right => unsafe { EVENTS_COUNTER.right_clicks += 1 },
                    rdev::Button::Middle => unsafe { EVENTS_COUNTER.middle_clicks += 1 },
                    _ => {}
                },

                rdev::EventType::KeyPress(key) => match key {
                    _ => unsafe { EVENTS_COUNTER.keys_pressed += 1 },
                },

                rdev::EventType::MouseMove { x, y } => unsafe {
                    if LAST_X_PX != 0.0 {
                        let mouse_dpi = 1600.0;
                        let power_x: f64 = ((LAST_Y_PX - y).powf(2.0)) / mouse_dpi;
                        let power_y: f64 = ((LAST_X_PX - x).powf(2.0)) / mouse_dpi;
                        let pixels_moved = (power_x + power_y).sqrt();
                        EVENTS_COUNTER.pixels_moved += pixels_moved.ceil();
                    }

                    LAST_X_PX = x;
                    LAST_Y_PX = y;
                },

                _ => {}
            }
        }
    }

    #[allow(dead_code)]
    pub fn print_counters(&mut self) {
        println!("{:#?}", self);
    }
}
