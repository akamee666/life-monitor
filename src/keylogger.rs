use crate::localdb::{get_input_data, open_con, send_to_input_table};
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

#[derive(Debug, Clone)]
pub struct KeyLogger {
    pub left_clicks: u64,
    pub right_clicks: u64,
    pub middle_clicks: u64,
    pub keys_pressed: u64,
    pub pixels_moved: f64,
    pub mouse_moved_cm: u64,
    pub mouse_dpi: f64,
    pub calibration_distance_cm: f64,
    pub calibration_pixels: f64,
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
        // Initialize with default values
        d.mouse_dpi = 1000.0; // Default DPI
        d.calibration_distance_cm = 10.0; // Default calibration distance
        d.calibration_pixels = 0.0; // Will be set during calibration

        d
    }

    fn update_mouse_movement(&mut self, delta_x: f64, delta_y: f64) {
        let pixels_moved = (delta_x.powf(2.0) + delta_y.powf(2.0)).sqrt();
        self.pixels_moved += pixels_moved;
    }

    // I googled and copied the first formula that i saw, i dont know how much accurate is it.
    fn calibrate(&mut self, pixels_moved: f64) {
        self.calibration_pixels = pixels_moved;
        self.mouse_dpi = (pixels_moved / self.calibration_distance_cm) * 2.54;
        info!("Mouse calibrated. DPI: {}", self.mouse_dpi);
    }

    fn to_cm(&mut self) {
        let cm = (self.pixels_moved / self.mouse_dpi) * 2.54;
        self.mouse_moved_cm = cm as u64;
    }
}

pub struct MousePosition {
    x: f64,
    y: f64,
}

impl MousePosition {
    fn new() -> Self {
        MousePosition { x: 0.0, y: 0.0 }
    }

    fn update(&mut self, x: f64, y: f64) -> (f64, f64) {
        let delta_x = self.x - x;
        let delta_y = self.y - y;
        self.x = x;
        self.y = y;
        (delta_x, delta_y)
    }
}

pub async fn init() {
    debug!("Keylogger spawned!");

    // FIX: Only if dpi is not provided or is zero,
    //calibrate_mouse().await;

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
        let mut interval = interval(Duration::from_secs(300));
        loop {
            interval.tick().await;

            // Acquire read lock to send data to the DB
            let mut guard = EVENTS_COUNTER.write().unwrap();
            guard.to_cm();
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
    // Do i really need to start this everytime? Is the old me dumb? But I guess there is no problem since they are only
    // two floats.
    let mut mousepos = MousePosition::new();
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
            let (delta_x, delta_y) = mousepos.update(x, y);
            if delta_x != 0.0 || delta_y != 0.0 {
                guard.update_mouse_movement(delta_x, delta_y);
            }
        }
        _ => {}
    }
}

// I suck at math so i just ask to claude do this, as everything in this code i do know expect the
// perfect accuracy from this but i guess that's better than guessing your dpi.
async fn calibrate_mouse() {
    println!("Try moving your mouse around 10 cm to any direction and press any key after that.");
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut start_pos = None;
    let mut end_pos = None;

    let _listener = tokio::spawn(async move {
        listen(move |event| {
            tx.send(event).unwrap_or_else(|e| {
                error!("Could not send event by bounded channel. err: {:?}", e)
            });
        })
        .expect("Could not listen to keys");
    });

    while let Some(event) = rx.recv().await {
        match event.event_type {
            rdev::EventType::MouseMove { x, y } => {
                if start_pos.is_none() {
                    start_pos = Some((x, y));
                }
                end_pos = Some((x, y));
            }
            rdev::EventType::KeyPress(_) => {
                if let (Some(start), Some(end)) = (start_pos, end_pos) {
                    let pixels_moved =
                        ((end.0 - start.0).powf(2.0) + (end.1 - start.1).powf(2.0)).sqrt();
                    let mut guard = EVENTS_COUNTER.write().unwrap();
                    guard.calibrate(pixels_moved);
                    break;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_keylogger_new() {
        let keylogger = KeyLogger::new();
        assert_eq!(keylogger.left_clicks, 0);
        assert_eq!(keylogger.right_clicks, 0);
        assert_eq!(keylogger.middle_clicks, 0);
        assert_eq!(keylogger.keys_pressed, 0);
        assert_eq!(keylogger.pixels_moved, 0.0);
        assert_eq!(keylogger.mouse_dpi, 1000.0);
        assert_eq!(keylogger.calibration_distance_cm, 10.0);
        assert_eq!(keylogger.calibration_pixels, 0.0);
    }

    #[test]
    fn test_update_mouse_movement() {
        let mut keylogger = KeyLogger::new();
        keylogger.update_mouse_movement(3.0, 4.0);
        assert_relative_eq!(keylogger.pixels_moved, 5.0);
        keylogger.update_mouse_movement(-3.0, -4.0);
        assert_relative_eq!(keylogger.pixels_moved, 10.0);
    }

    #[test]
    fn test_calibrate() {
        let mut keylogger = KeyLogger::new();
        keylogger.calibrate(1000.0);
        assert_eq!(keylogger.calibration_pixels, 1000.0);
        assert_relative_eq!(keylogger.mouse_dpi, 254.0);
    }

    #[test]
    fn test_to_cm() {
        let mut keylogger = KeyLogger::new();
        keylogger.mouse_dpi = 254.0; // Set DPI to 254 (100 pixels per cm)
        keylogger.pixels_moved = 1000.0;
        keylogger.to_cm();
        assert_eq!(keylogger.mouse_moved_cm, 10.0 as u64);
    }

    #[test]
    fn test_mouse_position() {
        let mut pos = MousePosition::new();
        assert_eq!(pos.x, 0.0);
        assert_eq!(pos.y, 0.0);

        let (dx, dy) = pos.update(3.0, 4.0);
        assert_eq!(dx, 3.0);
        assert_eq!(dy, 4.0);
        assert_eq!(pos.x, 3.0);
        assert_eq!(pos.y, 4.0);

        let (dx, dy) = pos.update(1.0, 1.0);
        assert_eq!(dx, -2.0);
        assert_eq!(dy, -3.0);
        assert_eq!(pos.x, 1.0);
        assert_eq!(pos.y, 1.0);
    }

    #[tokio::test]
    async fn test_handle_event() {
        let keylogger = KeyLogger::new();
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
        assert_relative_eq!(EVENTS_COUNTER.read().unwrap().pixels_moved, 5.0);
    }
}
