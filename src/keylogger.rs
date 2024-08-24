use crate::db::{get_input_data, send_to_input_table};
use once_cell::sync::Lazy;
use rdev::listen;
use std::{sync::Mutex, thread};
use tokio::{
    sync::mpsc,
    time::{interval, Duration},
};
use tracing::{debug, error};

static EVENTS_COUNTER: Lazy<Mutex<KeyLogger>> = Lazy::new(|| Mutex::new(KeyLogger::new()));

static mut LAST_X_PX: f64 = 0.0;
static mut LAST_Y_PX: f64 = 0.0;

#[derive(Debug, Copy, Clone)]
pub struct KeyLogger {
    pub left_clicks: u64,
    pub right_clicks: u64,
    pub middle_clicks: u64,
    pub keys_pressed: u64,
    pub pixels_moved: f64,
    pub mouse_moved_cm: u64,
}

impl KeyLogger {
    fn new() -> KeyLogger {
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
        let initial_data = get_input_data().unwrap_or_else(|_| KeyLogger::new());

        {
            let mut guard = EVENTS_COUNTER.lock().expect("poisoned");

            *guard = initial_data;
        }

        // thanks rdev devs for leave this beautiful example available. i was struggling to much to
        // find a way to not blocking everyhing and also have a timer.
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

                {
                    let mut guard = EVENTS_COUNTER.lock().expect("poisoned");
                    guard.mouse_moved_cm = ((guard.pixels_moved * 0.026) / 3.0) as u64;

                    if let Err(e) = send_to_input_table(&guard) {
                        error!("Error sending data to input table. Error: {e:?}");
                    }
                }
            }
        });

        loop {
            let event = rx.recv().await.expect("could not receive the event?");

            {
                let mut guard = EVENTS_COUNTER.lock().expect("poisoned");

                match event.event_type {
                    rdev::EventType::ButtonPress(button) => match button {
                        rdev::Button::Left => guard.left_clicks += 1,
                        rdev::Button::Right => guard.right_clicks += 1,
                        rdev::Button::Middle => guard.middle_clicks += 1,
                        _ => {}
                    },

                    rdev::EventType::KeyPress(_) => guard.keys_pressed += 1,

                    rdev::EventType::MouseMove { x, y } => {
                        if unsafe { LAST_X_PX != 0.0 } {
                            let mouse_dpi = 1600.0;
                            let power_x: f64 = ((unsafe { LAST_Y_PX } - y).powf(2.0)) / mouse_dpi;
                            let power_y: f64 = ((unsafe { LAST_X_PX } - x).powf(2.0)) / mouse_dpi;
                            let pixels_moved = (power_x + power_y).sqrt();
                            guard.pixels_moved += pixels_moved.ceil();
                        }

                        unsafe {
                            LAST_X_PX = x;
                            LAST_Y_PX = y;
                        }
                    }

                    _ => {}
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn print_counters(&mut self) {
        println!("{:#?}", self);
    }
}
