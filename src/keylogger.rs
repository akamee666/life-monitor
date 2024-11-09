#[cfg(target_os = "linux")]
use crate::platform::linux::util::*;
#[cfg(target_os = "windows")]
use crate::win::util::*;

use crate::backend::{DataStore, StorageBackend};
use crate::spawn_ticker;
use crate::Event;

use rdev::listen;
use serde::Deserialize;

use std::sync::Arc;

use tokio::sync::mpsc::channel;
use tokio::sync::Mutex;
use tokio::time::Duration;

use tracing::*;

#[derive(Debug, Clone, Deserialize)]
pub struct KeyLogger {
    pub t_lc: u64,
    pub t_rc: u64,
    pub t_mc: u64,
    pub t_kp: u64,
    #[serde(default)]
    pub pixels_moved: f64,
    #[serde(default)]
    pub t_mm: f64,
    #[serde(default)]
    pub last_pos: Option<(f64, f64)>,
    #[serde(default)]
    pub mouse_settings: MouseSettings,
}

impl Default for KeyLogger {
    fn default() -> Self {
        Self {
            t_lc: 0,
            t_rc: 0,
            t_mc: 0,
            t_kp: 0,
            pixels_moved: 0.0,
            t_mm: 0.0,
            last_pos: None,
            mouse_settings: MouseSettings::default(),
        }
    }
}

impl KeyLogger {
    async fn new(backend: &StorageBackend, dpi_arg: Option<u32>) -> Self {
        let mut k: Self = backend.get_keys_data().await.unwrap_or_else(|err| {
            error!("Call to backend to get keys data failed, quitting!\nError: {err}",);
            panic!();
        });

        // Default: speed: 0, mouse_params: [6, 10, 1], enhanced_pointer_precision: false.
        let s: MouseSettings = match get_mouse_settings() {
            Ok(mut settings) => {
                if let Some(dpi) = dpi_arg {
                    settings.dpi = dpi;
                }
                settings
            }

            Err(e) => {
                warn!("Error requesting mouse acceleration, using Default values!\nError: {e}");
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
        self.t_mm += cm;
        self.pixels_moved = 0.0;
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

pub async fn init(dpi_arg: Option<u32>, interval: Option<u32>, backend: StorageBackend) {
    let db_int = if let Some(interval) = interval {
        info!("Interval argument provided, changing values.");
        interval
    } else {
        300
    };

    let logger = Arc::new(Mutex::new(KeyLogger::new(&backend, dpi_arg).await));
    let (tx_t, mut rx_t) = channel(20);

    spawn_ticker(
        tx_t.clone(),
        Duration::from_secs(db_int.into()),
        Event::DbUpdate,
    );

    let logger_db = logger.clone();

    tokio::spawn(async move {
        while let Some(event) = rx_t.recv().await {
            if let Event::DbUpdate = event {
                let mut guard = logger_db.lock().await;
                guard.update_to_cm();

                if let Err(e) = backend.store_keys_data(&guard).await {
                    error!("Call to backend to store keys data failed.\nError: {e}");
                }
            }
        }
    });

    info!("Interval for database updates is: {} seconds.", db_int);

    let (tx, mut rx) = channel(300);

    // I am not sure if is the right choice call spawn_blocking here but it seems to be because
    // listen is not async.
    // https://stackoverflow.com/questions/63363513/sync-async-interoperable-channels
    // https://ryhl.io/blog/async-what-is-blocking/
    tokio::task::spawn_blocking(move || {
        listen(move |event| {
            tx.blocking_send(event).unwrap_or_else(|err| {
                error!("Could not send event by bounded channel.\nError: {err}");
            });
            //debug!("Event sent.");
        })
        .expect("Could not listen to keys");
    });

    // Wait until receive a event from the task above to compute it.
    while let Some(event) = rx.recv().await {
        //println!("Received {:?}", event);
        handle_event(event, &logger).await;
    }
}

async fn handle_event(event: rdev::Event, logger: &Arc<Mutex<KeyLogger>>) {
    //debug!("{:?}", event);

    // This might hurt the performance if waiting for lock but since database updates are not so
    // often, shouldn't be a problem.
    let mut logger = logger.lock().await;

    match event.event_type {
        rdev::EventType::ButtonPress(button) => match button {
            rdev::Button::Left => logger.t_lc += 1,
            rdev::Button::Right => logger.t_rc += 1,
            rdev::Button::Middle => logger.t_mc += 1,
            // Rest doesn't matter.
            _ => {}
        },
        // Rest doesn't matter.
        rdev::EventType::KeyPress(_) => logger.t_kp += 1,
        rdev::EventType::MouseMove { x, y } => {
            logger.update_distance(x, y);
        }
        // Rest doesn't matter.
        _ => {}
    }
}
