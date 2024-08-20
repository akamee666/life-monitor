use crate::db::upload_data_to_db;
use log::debug;
use log::info;
use rdev::listen;
use std::thread;
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio::time::Duration;

// even if i read this data at the same time other thread is writing to it i dont think
// there will be a problem, unless there are weird memory bugs it will be updated after some time
// so maybe it's fine leave it that way.
static mut EVENTS_COUNTER: KeyLogger = KeyLogger::new();
static mut LAST_X_PX: f64 = 0.0;
static mut LAST_Y_PX: f64 = 0.0;

#[allow(dead_code)]
#[derive(Debug)]
pub struct KeyLogger {
    pub left_clicks: i64,
    pub right_clicks: i64,
    pub middle_clicks: i64,
    pub keys_pressed: i64,
    pub pixels_moved: f64,
    pub mouse_moved_cm: f64,
}

impl KeyLogger {
    const fn new() -> KeyLogger {
        let left_clicks = 0;
        let right_clicks = 0;
        let middle_clicks = 0;
        let keys_pressed = 0;
        let mouse_moved_cm = 0.0;
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
        info!("Spawned KeyLogger thread");
        // thanks rdev devs for have left this example.
        // tx = transmiter.
        // rx = receiver.
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _listener = thread::spawn(move || {
            listen(move |event| {
                tx.send(event)
                    .unwrap_or_else(|e| println!("Could not send event {:?}", e));
            })
            .expect("Could not listen");
        });

        tokio::spawn(async {
            let mut interval = interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                upload_data_to_db();
            }
        });

        loop {
            let event = rx.recv().await.expect("could not receive the event?");
            debug!("Received a event: {:?}", event.event_type);
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

                        // This should not be here and also it's not accurate for all devices for sure, the
                        // division with 3.0 seems accurate for my mouse but if i change for another
                        // one it just fuck up with everything but i couldn't find a way to make it
                        // works so.
                        EVENTS_COUNTER.mouse_moved_cm += (pixels_moved.ceil() * 0.026) / 3.0;
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
