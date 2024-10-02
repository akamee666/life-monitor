use crate::{linux::util::get_mouse_acceleration, localdb::*};
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
    pub mouse_dpi: u32,
    pub last_pos: Option<(f64, f64)>,
    pub numerator: u16,
    pub denominator: u16,
    pub threshold: u16,
}

impl KeyLogger {
    fn new() -> Self {
        let con = open_con().unwrap_or_else(|err| {
            error!(
                "Could not open a connection with local database, quitting! Err: {:?}",
                err
            );
            panic!(
                "Could not open a connection with local database, quitting! Err: {:?}",
                err
            );
        });
        let mut k = get_input_data(&con).unwrap_or_else(|err| {
            error!(
                "Could not open a connection with local database, quitting! Err: {:?}",
                err
            );
            panic!(
                "Could not open a connection with local database, quitting! Err: {:?}",
                err
            );
        });

        match get_mouse_acceleration() {
            Ok((n, d, t)) => {
                k.threshold = t;
                k.numerator = n;
                k.denominator = d;
            }
            Err(e) => {
                error!("Error requesting mouse acceleration! Err: {e}")
            }
        }

        k
    }

    fn update_delta(&mut self, x: f64, y: f64) {
        // Only calculate distance if we have a previous position
        if let Some((last_x, last_y)) = self.last_pos {
            let distance_moved = ((last_x - x).powi(2) + (last_y - y).powi(2)).sqrt();
            // Apply acceleration only to the portion exceeding the threshold
            let adjusted_distance = if distance_moved > self.threshold as f64 {
                let accel_factor = self.numerator as f64 / self.denominator as f64;
                let base_distance = self.threshold as f64;
                let accelerated_distance = (distance_moved - base_distance) * accel_factor;
                base_distance + accelerated_distance
            } else {
                distance_moved
            };

            // Accumulate the adjusted distance in pixels
            self.pixels_moved += adjusted_distance;
        }

        // Update last position
        self.last_pos = Some((x, y))
    }

    fn update_to_cm(&mut self) {
        let inches = self.pixels_moved / self.mouse_dpi as f64;
        let cm = inches * 2.54;
        self.mouse_moved_cm += cm;
        self.pixels_moved = 0.0;

        debug!("Mouse moved cm: {}", self.mouse_moved_cm);
    }

    fn get_args(&mut self, dpi: u32) {
        self.mouse_dpi = dpi;
    }
}

pub async fn init(dpi_arg: u32) {
    debug!("Keylogger spawned!");
    let mut guard = EVENTS_COUNTER.write().unwrap();
    if guard.mouse_dpi == 0 {
        guard.get_args(dpi_arg);
        match get_mouse_acceleration() {
            Ok((n, d, t)) => {
                guard.denominator = d;
                guard.threshold = t;
                guard.numerator = n;
            }
            Err(e) => {
                error!("Could not request mouse acceleration settings. Err:{e}");
            }
        }
    }
    drop(guard);

    // Periodic task for sending data to the DB every 5 minutes.
    tokio::spawn(async {
        let con = open_con().unwrap_or_else(|err| {
            error!(
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
    fn test_update_delta() {
        // Test case 1: No acceleration (numerator = denominator = 1, threshold = 0)
        let mut logger = KeyLogger {
            numerator: 1,
            denominator: 1,
            threshold: 0,
            ..Default::default()
        };

        // Initial move
        logger.update_delta(3.0, 4.0);
        assert_relative_eq!(logger.pixels_moved, 0.0); // First move, no change

        // Second move
        logger.update_delta(0.0, 0.0);
        assert_relative_eq!(logger.pixels_moved, 5.0); // Pythagorean theorem: sqrt(3^2 + 4^2) = 5

        // Test case 2: With acceleration (numerator = 2, denominator = 1, threshold = 10)
        let mut logger_accel = KeyLogger {
            numerator: 2,
            denominator: 1,
            threshold: 10,
            ..Default::default()
        };

        // Move below threshold
        logger_accel.update_delta(4.0, 5.0);
        logger_accel.update_delta(1.0, 1.0);
        assert_relative_eq!(logger_accel.pixels_moved, 5.0); // No acceleration applied

        // Move above threshold
        logger_accel.update_delta(21.0, 1.0);
        assert_relative_eq!(logger_accel.pixels_moved, 35.0); // 5 + (20 - 10) * 2 + 10 = 35

        // Test case 3: Different start and end points
        let mut logger_diff = KeyLogger {
            numerator: 1,
            denominator: 1,
            threshold: 0,
            ..Default::default()
        };

        logger_diff.update_delta(10.0, 10.0);
        logger_diff.update_delta(13.0, 14.0);
        assert_relative_eq!(logger_diff.pixels_moved, 5.0); // sqrt((13-10)^2 + (14-10)^2) = 5

        // Test case 4: Negative coordinates
        let mut logger_neg = KeyLogger {
            numerator: 1,
            denominator: 1,
            threshold: 0,
            ..Default::default()
        };

        logger_neg.update_delta(-3.0, -4.0);
        logger_neg.update_delta(0.0, 0.0);
        assert_relative_eq!(logger_neg.pixels_moved, 5.0);

        // Test case 5: Very small movements
        let mut logger_small = KeyLogger {
            numerator: 1,
            denominator: 1,
            threshold: 0,
            ..Default::default()
        };

        logger_small.update_delta(0.1, 0.1);
        logger_small.update_delta(0.2, 0.2);
        assert_relative_eq!(logger_small.pixels_moved, 0.1414, epsilon = 0.0001);
    }

    #[test]
    fn test_to_cm() {
        let mut keylogger = KeyLogger {
            numerator: 1,
            denominator: 1,
            threshold: 0,
            ..Default::default()
        };
        keylogger.mouse_dpi = 800;
        keylogger.pixels_moved = 1000.0;
        keylogger.update_to_cm();
        assert_relative_eq!(keylogger.mouse_moved_cm, 3.175, epsilon = 0.001);

        // Test with different DPI
        keylogger.mouse_dpi = 1600;
        keylogger.pixels_moved = 1000.0;
        keylogger.update_to_cm();
        assert_relative_eq!(keylogger.mouse_moved_cm, 1.5875, epsilon = 0.001);
    }

    #[test]
    fn test_mouse_accuracy() {
        let mut keylogger = KeyLogger {
            numerator: 1,
            denominator: 1,
            threshold: 0,
            ..Default::default()
        };
        keylogger.mouse_dpi = 800;

        // Test diagonal movement
        keylogger.update_delta(0.0, 0.0);
        keylogger.update_delta(1920.0, 1080.0);
        assert_relative_eq!(keylogger.pixels_moved, 2203.3608, max_relative = 1.0);
        keylogger.update_to_cm();
        assert_relative_eq!(keylogger.mouse_moved_cm, 7.0, epsilon = 0.1);

        // Test horizontal movement
        let mut keylogger = KeyLogger {
            numerator: 1,
            denominator: 1,
            threshold: 0,
            ..Default::default()
        };

        keylogger.mouse_dpi = 800;
        keylogger.update_delta(0.0, 0.0);
        keylogger.update_delta(1920.0, 0.0);
        assert_relative_eq!(keylogger.pixels_moved, 1920.0, epsilon = 0.001);
        keylogger.update_to_cm();
        assert_relative_eq!(keylogger.mouse_moved_cm, 6.096, epsilon = 0.001);
    }

    #[test]
    fn test_mouse_acceleration() {
        let mut keylogger = KeyLogger {
            numerator: 2,
            denominator: 1,
            threshold: 10,
            ..Default::default()
        };

        // Movement below threshold
        keylogger.update_delta(0.0, 0.0);
        keylogger.update_delta(5.0, 0.0);
        assert_relative_eq!(keylogger.pixels_moved, 5.0, epsilon = 0.001);

        // Movement above threshold
        keylogger.update_delta(25.0, 0.0);
        let expected = 5.0 + 10.0 + (10.0 * 2.0); // Initial + Threshold + Accelerated
        assert_relative_eq!(keylogger.pixels_moved, expected, epsilon = 0.001);
    }

    #[tokio::test]
    async fn test_handle_event() {
        let keylogger = KeyLogger {
            numerator: 1,
            denominator: 1,
            threshold: 0,
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
