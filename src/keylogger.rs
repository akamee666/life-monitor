#[cfg(target_os = "linux")]
use crate::linux::util::{get_mouse_settings, MouseSettings};

#[cfg(target_os = "windows")]
use crate::win::util::{get_mouse_settings, MouseSettings};

use crate::localdb::*;
use once_cell::sync::Lazy;
use rdev::listen;
use std::sync::{Arc, RwLock};
use std::thread;
use tokio::{
    sync::mpsc::{self, channel},
    time::{interval, Duration},
};
use tracing::*;

// RwLock for read/write access to KeyLogger
static EVENTS_COUNTER: Lazy<Arc<RwLock<KeyLogger>>> =
    Lazy::new(|| Arc::new(RwLock::new(KeyLogger::new())));

#[derive(Debug, Default)]
pub struct KeyLogger {
    pub left_clicks: u64,
    pub right_clicks: u64,
    pub middle_clicks: u64,
    pub keys_pressed: u64,
    pub pixels_moved: f64,
    pub mouse_moved_cm: f64,
    pub last_pos: Option<(f64, f64)>,
    pub mouse_settings: MouseSettings,
}

#[derive(Clone, Copy, Debug)]
enum Event {
    DbUpdate,
}

impl KeyLogger {
    fn new() -> Self {
        // Get values stored in database, open_con already check if there is a database to get data
        // from.
        let con = open_con().unwrap_or_else(|err| {
            error!(
                "Could not open a connection with local database for keys table, quitting!\n Err: {:?}",
                err
            );
            panic!();
        });

        let mut k = get_keyst(&con).unwrap_or_else(|err| {
            error!(
                "Connection with the keys table was opened but could not receive data from table, quitting!\n Err: {:?}",
                err
            );
            panic!();
        });

        // speed: 0, mouse_params: [6, 10, 1], enhanced_pointer_precision: false.
        let s: MouseSettings = match get_mouse_settings() {
            Ok(settings) => settings,

            Err(e) => {
                warn!("Error requesting mouse acceleration, using Default values! Err: {e}");
                MouseSettings {
                    ..Default::default()
                }
            }
        };

        k.mouse_settings = s;
        k
    }

    fn update_to_cm(&mut self) {
        let inches = self.pixels_moved / self.mouse_settings.dpi as f64;
        let cm = inches * 2.54;
        self.mouse_moved_cm += cm;
        self.pixels_moved = 0.0;

        debug!("Mouse moved cm: {}", self.mouse_moved_cm);
    }

    #[cfg(target_os = "linux")]
    fn update_distance(&mut self, x: f64, y: f64) {
        // Only calculate distance if we have a previous position
        if let Some((last_x, last_y)) = self.last_pos {
            let distance_moved = ((last_x - x).powi(2) + (last_y - y).powi(2)).sqrt();
            // Apply acceleration only to the portion exceeding the threshold
            let adjusted_distance = if distance_moved > self.mouse_settings.threshold as f64 {
                let accel_factor = self.mouse_settings.acceleration_numerator as f64
                    / self.mouse_settings.acceleration_denominator as f64;
                let base_distance = self.mouse_settings.threshold as f64;
                let accelerated_distance = (distance_moved - base_distance) * accel_factor;
                base_distance + accelerated_distance
            } else {
                distance_moved
            };
            //debug!("adjusted_distance: {adjusted_distance}");
            // Accumulate the adjusted distance in pixels
            self.pixels_moved += adjusted_distance;
        }

        // Update last position
        self.last_pos = Some((x, y))
    }

    #[cfg(target_os = "windows")]
    fn update_distance(&mut self, x: f64, y: f64) {
        if let Some((last_x, last_y)) = self.last_pos {
            let distance_moved = ((last_x - x).powi(2) + (last_y - y).powi(2)).sqrt();

            let adjusted_distance = if self.mouse_settings.enhanced_pointer_precision {
                self.apply_windows_acceleration(distance_moved)
            } else {
                distance_moved
            };

            self.pixels_moved += adjusted_distance;
        }
        self.last_pos = Some((x, y));
    }

    #[cfg(target_os = "windows")]
    fn apply_windows_acceleration(&self, distance: f64) -> f64 {
        let speed = distance; // Assume distance is proportional to speed
        let threshold1 = self.mouse_settings.threshold as f64;
        let threshold2 = self.mouse_settings.threshold2 as f64;
        let acceleration = self.mouse_settings.acceleration as f64;

        if speed > threshold2 {
            distance * acceleration
        } else if speed > threshold1 {
            let t = (speed - threshold1) / (threshold2 - threshold1);
            let accel_factor = 1.0 + t * (acceleration - 1.0);
            distance * accel_factor
        } else {
            distance
        }
    }
}

fn spawn_ticker(tx: mpsc::Sender<Event>, duration: Duration, event: Event) {
    tokio::spawn(async move {
        let mut interval = interval(duration);
        loop {
            interval.tick().await;
            if tx.send(event).await.is_err() {
                break;
            }
        }
    });
}

pub async fn init(dpi_arg: Option<u32>, interval: Option<u32>) {
    let mut db_interval = 300;

    if interval.is_some() {
        info!("Interval argument provided, changing values.");
        db_interval = interval.unwrap();
    }

    if dpi_arg.is_some() {
        info!("Dpi argument provided, changing values.");
        let mut guard = EVENTS_COUNTER.write().unwrap();
        guard.mouse_settings.dpi = dpi_arg.unwrap();
    }

    let con = open_con().unwrap_or_else(|err| {
        error!(
            "Could not open a connection with local database, quitting! Err: {:?}",
            err
        );
        panic!();
    });

    info!("Connection with the database for Keylogger is open");

    let (tx_ticker, mut rx_ticker) = channel(100);
    spawn_ticker(
        tx_ticker.clone(),
        Duration::from_secs(db_interval.into()),
        Event::DbUpdate,
    );

    let db_update_task = tokio::spawn(async move {
        while let Some(event) = rx_ticker.recv().await {
            match event {
                Event::DbUpdate => {
                    // WARN: Write lock is used here cause casting every time i receive a event is more
                    // expensive. But locking write here means we need to wait until the database
                    // operation is done to write to it again. Maybe that doesn't matter here, cause of
                    // the channels has a buffer as far i know? not sure though.

                    debug!("Database event tick, sending data from Keylogger now.");
                    let mut guard = EVENTS_COUNTER.write().unwrap();
                    guard.update_to_cm();
                    drop(guard);
                    let guard = EVENTS_COUNTER.read().unwrap();
                    if let Err(e) = update_keyst(&con, &guard) {
                        error!("Error sending data to input table. Error: {e:?}");
                    }
                }
            }
        }
    });

    info!(
        "Ticker for database updates created, interval is:[{}]",
        db_interval
    );

    // blocking.
    info!("Database update task spawned!");

    let (tx, mut rx) = mpsc::unbounded_channel();

    // For some reason, using tokio threads does not work, events are sent by the channel but rx
    // receives nothing.
    let _listener = thread::spawn(move || {
        debug!("Starting event listener");
        listen(move |event| {
            //debug!("Captured event: {:?}", event);
            if let Err(e) = tx.send(event) {
                error!("Could not send event through channel. err: {:?}", e);
            } else {
                //debug!("Event sent through channel successfully");
            }
        })
        .expect("Could not listen to keys");
    });

    // Wait until receive a event from the task above to compute it.
    while let Some(event) = rx.recv().await {
        handle_event(event).await;
    }

    // will not end.
    db_update_task.await.unwrap();
}

async fn handle_event(event: rdev::Event) {
    //debug!("{:?}", event);
    let mut guard = EVENTS_COUNTER.write().unwrap();

    // Basically the code just increment depending on the event type.
    match event.event_type {
        rdev::EventType::ButtonPress(button) => match button {
            rdev::Button::Left => guard.left_clicks += 1,
            rdev::Button::Right => guard.right_clicks += 1,
            rdev::Button::Middle => guard.middle_clicks += 1,
            // Rest doesn't matter.
            _ => {}
        },
        // Rest doesn't matter.
        rdev::EventType::KeyPress(_) => guard.keys_pressed += 1,
        rdev::EventType::MouseMove { x, y } => {
            guard.update_distance(x, y);
        }
        // Rest doesn't matter.
        _ => {}
    }
}

// FIX: Create for both os.
#[cfg(target_os = "linux")]
#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_update_distance() {
        // Test case 1: No acceleration (mouse_settings.numerator = denominator = 1, threshold = 0)
        let mut logger = KeyLogger {
            mouse_settings: MouseSettings::noacc_default(),
            ..Default::default()
        };

        // Initial move
        logger.update_distance(3.0, 4.0);
        assert_relative_eq!(logger.pixels_moved, 0.0); // First move, no change

        // Second move
        logger.update_distance(0.0, 0.0);
        assert_relative_eq!(logger.pixels_moved, 5.0); // Pythagorean theorem: sqrt(3^2 + 4^2) = 5

        // Test case 2: With acceleration (mouse_settings.numerator = 2, denominator = 1, threshold = 10)
        let mut logger_accel = KeyLogger {
            mouse_settings: MouseSettings::default(),
            ..Default::default()
        };

        // Move below threshold
        logger_accel.update_distance(4.0, 5.0);
        logger_accel.update_distance(1.0, 1.0);
        assert_relative_eq!(logger_accel.pixels_moved, 7.0); // No acceleration applied

        // Move above threshold
        logger_accel.update_distance(21.0, 1.0);
        assert_relative_eq!(logger_accel.pixels_moved, 35.0); // 5 + (20 - 10) * 2 + 10 = 35

        // Test case 3: Different start and end points
        let mut logger_diff = KeyLogger {
            mouse_settings: MouseSettings::noacc_default(),
            ..Default::default()
        };

        logger_diff.update_distance(10.0, 10.0);
        logger_diff.update_distance(13.0, 14.0);
        assert_relative_eq!(logger_diff.pixels_moved, 5.0); // sqrt((13-10)^2 + (14-10)^2) = 5

        // Test case 4: Negative coordinates
        let mut logger_neg = KeyLogger {
            mouse_settings: MouseSettings::noacc_default(),
            ..Default::default()
        };

        logger_neg.update_distance(-3.0, -4.0);
        logger_neg.update_distance(0.0, 0.0);
        assert_relative_eq!(logger_neg.pixels_moved, 5.0);

        // Test case 5: Very small movements
        let mut logger_small = KeyLogger {
            mouse_settings: MouseSettings::noacc_default(),
            ..Default::default()
        };

        logger_small.update_distance(0.1, 0.1);
        logger_small.update_distance(0.2, 0.2);
        assert_relative_eq!(logger_small.pixels_moved, 0.1414, epsilon = 0.0001);
    }

    #[test]
    fn test_to_cm() {
        let mut keylogger = KeyLogger {
            mouse_settings: MouseSettings::noacc_default(),
            ..Default::default()
        };
        keylogger.mouse_settings.dpi = 800;
        keylogger.pixels_moved = 1000.0;
        keylogger.update_to_cm();
        assert_relative_eq!(keylogger.mouse_moved_cm, 3.175, epsilon = 0.001);

        // Test with different DPI
        keylogger.mouse_settings.dpi = 1600;
        keylogger.pixels_moved = 1000.0;

        // Reset the value from before.
        keylogger.mouse_moved_cm = 0.0;
        keylogger.update_to_cm();
        assert_relative_eq!(keylogger.mouse_moved_cm, 1.5875, epsilon = 0.001);
    }

    #[test]
    fn test_mouse_accuracy() {
        let mut keylogger = KeyLogger {
            mouse_settings: MouseSettings::noacc_default(),
            ..Default::default()
        };
        keylogger.mouse_settings.dpi = 800;

        // Test diagonal movement
        keylogger.update_distance(0.0, 0.0);
        keylogger.update_distance(1920.0, 1080.0);
        assert_relative_eq!(keylogger.pixels_moved, 2203.3608, max_relative = 1.0);
        keylogger.update_to_cm();
        assert_relative_eq!(keylogger.mouse_moved_cm, 7.0, epsilon = 0.1);

        // Test horizontal movement
        let mut keylogger = KeyLogger {
            mouse_settings: MouseSettings::noacc_default(),
            ..Default::default()
        };

        keylogger.mouse_settings.dpi = 800;
        keylogger.update_distance(0.0, 0.0);
        keylogger.update_distance(1920.0, 0.0);
        assert_relative_eq!(keylogger.pixels_moved, 1920.0, epsilon = 0.001);
        keylogger.update_to_cm();
        assert_relative_eq!(keylogger.mouse_moved_cm, 6.096, epsilon = 0.001);
    }

    #[test]
    fn test_mouse_acceleration() {
        let mut keylogger = KeyLogger {
            mouse_settings: MouseSettings {
                acceleration_numerator: 2,
                acceleration_denominator: 1,
                threshold: 10,
                dpi: 800,
            },
            ..Default::default()
        };

        // Movement below threshold
        keylogger.update_distance(0.0, 0.0);
        keylogger.update_distance(5.0, 0.0);
        assert_relative_eq!(keylogger.pixels_moved, 5.0, epsilon = 0.001);

        // Movement above threshold
        keylogger.update_distance(25.0, 0.0);
        let expected = 5.0 + 10.0 + (10.0 * 2.0); // Initial + Threshold + Accelerated
        assert_relative_eq!(keylogger.pixels_moved, expected, epsilon = 0.001);
    }

    #[tokio::test]
    async fn test_handle_event() {
        let keylogger = KeyLogger {
            mouse_settings: MouseSettings::noacc_default(),
            ..Default::default()
        };

        *EVENTS_COUNTER.write().unwrap() = keylogger;

        // Test left click
        handle_event(rdev::Event {
            event_type: rdev::EventType::ButtonPress(rdev::Button::Left),
            time: std::time::SystemTime::now(),
            name: None,
        })
        .await;
        assert_eq!(EVENTS_COUNTER.read().unwrap().left_clicks, 1);

        // Test right click
        handle_event(rdev::Event {
            event_type: rdev::EventType::ButtonPress(rdev::Button::Right),
            time: std::time::SystemTime::now(),
            name: None,
        })
        .await;
        assert_eq!(EVENTS_COUNTER.read().unwrap().right_clicks, 1);

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
        handle_event(rdev::Event {
            event_type: rdev::EventType::MouseMove { x: 0.0, y: 0.0 },
            time: std::time::SystemTime::now(),
            name: None,
        })
        .await;
        assert_relative_eq!(
            EVENTS_COUNTER.read().unwrap().pixels_moved,
            5.0,
            epsilon = 0.001
        );
    }
}
