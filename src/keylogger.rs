use crate::localdb::*;
use once_cell::sync::Lazy;
use rdev::listen;
use std::sync::{Arc, RwLock};
use tokio::{
    sync::mpsc,
    time::{interval, Duration},
};
use tracing::*;

// RwLock for read/write access to KeyLogger
static EVENTS_COUNTER: Lazy<Arc<RwLock<KeyLogger>>> =
    Lazy::new(|| Arc::new(RwLock::new(KeyLogger::new())));

#[derive(Debug, Clone, Default)]
pub struct KeyLogger {
    pub left_clicks: u64,
    pub right_clicks: u64,
    pub middle_clicks: u64,
    pub keys_pressed: u64,
    pub pixels_moved: f64,
    pub mouse_moved_cm: f64,
    pub mouse_dpi: u64,
    pub last_x: f64,
    pub last_y: f64,
}

impl KeyLogger {
    fn new() -> Self {
        let con = open_con().unwrap_or_else(|err| {
            debug!(
                "Could not open a connection with local database, quitting! Err: {:?}",
                err
            );
            panic!(
                "Could not open a connection with local database, quitting! Err: {:?}",
                err
            );
        });

        let mut d = get_input_data(&con).unwrap_or_else(|err| {
            debug!(
                "Could not open a connection with local database, quitting! Err: {:?}",
                err
            );
            panic!(
                "Could not open a connection with local database, quitting! Err: {:?}",
                err
            );
        });

        // FIX: I NEED TO STORE THE DATA FROM DPI IN THE DATABASE SO I DO NOT ASK EVERYTIME I START
        // THE PROGRAM.
        d.mouse_dpi = 800; // Default DPI

        d
    }

    fn update_delta(&mut self, x: f64, y: f64) {
        // FIX: Remove from here.
        //match get_mouse_acceleration() {
        //    Ok((numerator, denominator, threshold)) => {
        //        info!("Mouse Acceleration: {}/{}", numerator, denominator);
        //        info!("Mouse Threshold: {}", threshold);
        //    }
        //    Err(e) => eprintln!("Failed to get mouse acceleration: {:?}", e),
        //}

        // Euclidean Distace.
        if self.last_x != 0.0 || self.last_y != 0.0 {
            let distance_moved = ((self.last_x - x).powi(2) + (self.last_y - y).powi(2)).sqrt();
            // If the movement exceeds the threshold, apply the acceleration factor
            let threshold = 4;
            let accel_denominator = 1;
            let accel_numerator = 2;
            let adjusted_distance = if distance_moved as i16 > threshold {
                let accel_factor = accel_numerator as f64 / accel_denominator as f64;
                (distance_moved - threshold as f64) * accel_factor + threshold as f64
            } else {
                distance_moved
            };

            // Accumulate the adjusted distance in pixels
            self.pixels_moved += adjusted_distance;
            self.update_to_cm();
        }

        self.last_x = x;
        self.last_y = y;
    }

    fn update_to_cm(&mut self) {
        let inches = self.pixels_moved / self.mouse_dpi as f64;
        let cm = inches * 2.54;
        self.mouse_moved_cm = cm;

        //info!("Mouse moved cm: {}", self.mouse_moved_cm);
        //info!("cm: {}", cm);
        //debug!("inches: {}", inches);
    }

    fn get_args(&mut self, dpi: u64) {
        self.mouse_dpi = dpi;
    }
}

pub async fn init(dpi_arg: u64) {
    debug!("Keylogger spawned!");

    let mut guard = EVENTS_COUNTER.write().unwrap();
    guard.get_args(dpi_arg);
    drop(guard);

    // Periodic task for sending data to the DB every 5 minutes.
    tokio::spawn(async {
        let con = open_con().unwrap_or_else(|err| {
            debug!(
                "Could not open a connection with local database, quitting! Err: {:?}",
                err
            );
            panic!(
                "Could not open a connection with local database, quitting! Err: {:?}",
                err
            );
        });
        let mut interval = interval(Duration::from_secs(5));

        loop {
            interval.tick().await;

            // Acquire read lock to send data to the DB
            let mut guard = EVENTS_COUNTER.write().unwrap();
            guard.update_to_cm();
            if let Err(e) = send_to_input_table(&con, &guard) {
                error!("Error sending data to input table. Error: {e:?}");
            }
        }
    });

    let (tx, mut rx) = mpsc::unbounded_channel();

    // As listen blocks, need a task/thread and channels.
    let _listener = tokio::spawn(async move {
        listen(move |event| {
            tx.send(event).unwrap_or_else(|e| {
                error!("Could not send event by bounded channel. err: {:?}", e)
            });
        })
        .expect("Could not listen to keys");
    });

    // Wait until receive a event from the task above to compute it.
    while let Some(event) = rx.recv().await {
        handle_event(event).await;
    }
}

async fn handle_event(event: rdev::Event) {
    let mut guard = EVENTS_COUNTER.write().unwrap();

    // Basically the code just increment depending on the event type.
    // If the mouse is the event type, the formula(that i just copy n paste from somewhere) calculates how much pixels the mouse has moved.
    match event.event_type {
        rdev::EventType::ButtonPress(button) => match button {
            rdev::Button::Left => guard.left_clicks += 1,
            rdev::Button::Right => guard.right_clicks += 1,
            rdev::Button::Middle => guard.middle_clicks += 1,
            _ => {}
        },
        rdev::EventType::KeyPress(_) => guard.keys_pressed += 1,
        rdev::EventType::MouseMove { x, y } => {
            guard.update_delta(x, y);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_calc_deta() {
        let mut keylogger = KeyLogger {
            ..Default::default()
        };

        keylogger.update_delta(3.0, 4.0);
        assert_relative_eq!(keylogger.pixels_moved, 7.0);
        keylogger.update_delta(0.0, 0.0);
        assert_relative_eq!(keylogger.pixels_moved, 14.0);
        keylogger.update_delta(0.0, 0.0);
        assert_relative_eq!(keylogger.pixels_moved, 14.0);
        keylogger.update_delta(-3.0, -4.0);
        assert_relative_eq!(keylogger.pixels_moved, 21.0);
        keylogger.update_delta(3.0, 4.0);
        assert_relative_eq!(keylogger.pixels_moved, 35.0);
    }

    #[test]
    fn test_to_cm() {
        // create real case test here.
        let mut keylogger = KeyLogger {
            ..Default::default()
        };
        keylogger.mouse_dpi = 800;
        keylogger.pixels_moved = 1000.0;
        keylogger.update_to_cm();
        assert_relative_eq!(keylogger.mouse_moved_cm, 26.0);
    }

    #[test]
    fn test_mouse_accuracy() {
        // create real case test here.
        let mut keylogger = KeyLogger {
            ..Default::default()
        };

        keylogger.mouse_dpi = 800;
        keylogger.update_delta(0.0, 0.0);
        keylogger.update_delta(1920.0, 1080.0);
        assert_relative_eq!(keylogger.pixels_moved, 3000.0);
        keylogger.update_to_cm();
        assert_relative_eq!(keylogger.mouse_moved_cm, 26.0 * 3 as f64);
    }

    #[tokio::test]
    async fn test_handle_event() {
        let keylogger = KeyLogger {
            ..Default::default()
        };

        *EVENTS_COUNTER.write().unwrap() = keylogger.clone();

        // Test left click
        handle_event(rdev::Event {
            event_type: rdev::EventType::ButtonPress(rdev::Button::Left),
            time: std::time::SystemTime::now(),
            name: None,
        })
        .await;
        assert_eq!(EVENTS_COUNTER.read().unwrap().left_clicks, 1);

        // Test key press
        handle_event(rdev::Event {
            event_type: rdev::EventType::KeyPress(rdev::Key::KeyA),
            time: std::time::SystemTime::now(),
            name: None,
        })
        .await;
        assert_eq!(EVENTS_COUNTER.read().unwrap().keys_pressed, 1);

        // Test mouse move
        handle_event(rdev::Event {
            event_type: rdev::EventType::MouseMove { x: 3.0, y: 4.0 },
            time: std::time::SystemTime::now(),
            name: None,
        })
        .await;
        assert_relative_eq!(EVENTS_COUNTER.read().unwrap().pixels_moved, 7.0);
    }
}
