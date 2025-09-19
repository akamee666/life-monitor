//! This file is responsible to store functions, enums or
//! structs that can be used for all platforms supported.
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tokio::time::Duration;

use anyhow::Context;
use anyhow::Result;

use crate::storage::backend::*;

use tracing::*;

use std::env;
use std::io::{self};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum Signals {
    Tick,
    DbUpdate,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "remote", derive(serde::Deserialize))]
pub struct ProcessInfo {
    pub w_name: String,
    pub w_time: u64,
    pub w_class: String,
}

#[derive(Debug, Default, Clone, PartialEq)]
#[cfg_attr(feature = "remote", derive(serde::Deserialize))]
pub struct InputLogger {
    /// Total number of left mouse button clicks.
    pub left_clicks: u64,
    /// Total number of right mouse button clicks.
    pub right_clicks: u64,
    /// Total number of middle mouse button clicks.
    pub middle_clicks: u64,
    /// Total number of keyboard key presses.
    pub key_presses: u64,
    /// Total distance the cursor has moved, measured in pixels.
    pub pixels_traveled: u64,
    /// Total distance the cursor has moved, measured in centimeters.
    pub cm_traveled: u64,
    /// DPI
    pub mouse_dpi: u64,
    /// Total number of raw vertical scroll wheel clicks.
    pub vertical_scroll_clicks: u64,
    /// Total number of raw horizontal scroll wheel clicks.
    pub horizontal_scroll_clicks: u64,
    /// Estimated vertical scroll distance in cm.
    pub vertical_scroll_cm: f64,
    /// Estimated horizontal scroll distance in cm.
    pub horizontal_scroll_cm: f64,
}

#[derive(Debug)]
pub struct ProcessTracker {
    pub time: u64,
    pub last_wname: String,
    pub last_wclass: String,
    pub procs: Vec<ProcessInfo>,
}

impl ProcessTracker {
    pub async fn new(backend: &StorageBackend) -> Result<Self> {
        let d: Vec<ProcessInfo> = backend.get_proc_data().await?;

        Ok(ProcessTracker {
            time: 0,
            last_wname: String::new(),
            last_wclass: String::new(),
            procs: d,
        })
    }
}

impl InputLogger {
    pub async fn new(backend: &StorageBackend, dpi: u32) -> Result<Self> {
        let mut k: Self = backend.get_keys_data().await.with_context(|| {
            "Failed to retrieve data from keys table to initialize keylogger struct"
        })?;
        k.mouse_dpi = dpi as u64;
        Ok(k)
    }

    // #[cfg(target_os = "windows")]
    // fn update_distance(&mut self, x: f64, y: f64) {
    //     if let Some((last_x, last_y)) = self.last_pos {
    //         let distance_moved = ((last_x - x).powi(2) + (last_y - y).powi(2)).sqrt();
    //
    //         let adjusted_distance = if self.mouse_settings.enhanced_pointer_precision {
    //             self.apply_windows_acceleration(distance_moved)
    //         } else {
    //             distance_moved
    //         };
    //
    //         self.pixels_moved += adjusted_distance;
    //     }
    //     self.last_pos = Some((x, y));
    // }
    //
    // #[cfg(target_os = "windows")]
    // fn apply_windows_acceleration(&self, distance: f64) -> f64 {
    //     let speed = distance; // Assume distance is proportional to speed
    //     let threshold1 = self.mouse_settings.threshold as f64;
    //     let threshold2 = self.mouse_settings.threshold2 as f64;
    //     let acceleration = self.mouse_settings.acceleration as f64;
    //
    //     if speed > threshold2 {
    //         distance * acceleration
    //     } else if speed > threshold1 {
    //         let t = (speed - threshold1) / (threshold2 - threshold1);
    //         let accel_factor = 1.0 + t * (acceleration - 1.0);
    //         distance * accel_factor
    //     } else {
    //         distance
    //     }
    // }
}

/// Spawns a new asynchronous task that sends a message on a channel at a regular interval.
pub fn spawn_ticker<T>(tx: mpsc::Sender<T>, duration: Duration, event_to_send: T) -> JoinHandle<()>
where
    T: Clone + Send + 'static,
{
    let join_handle = tokio::spawn(async move {
        let mut interval = interval(duration);
        interval.tick().await;
        loop {
            interval.tick().await;
            if tx.send(event_to_send.clone()).await.is_err() {
                error!("Ticker channel closed. Shutting down ticker task");
                break;
            }
        }
    });

    join_handle
}

/// Returns a platform-specific path for storing program-related files and ensures the directory exists.
///
/// On Windows: `%LOCALAPPDATA%\akame_monitor`  
/// On Linux: `$HOME/.local/share/akame_monitor`  
///
/// # Errors
/// Returns an error if the required environment variable is not set, if the OS is unsupported,
/// or if the directory cannot be created.
pub fn program_data_dir() -> io::Result<PathBuf> {
    // Determine the base directory for application data
    let base_dir = if cfg!(target_os = "windows") {
        env::var("LOCALAPPDATA").map(PathBuf::from).map_err(|_| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "LOCALAPPDATA environment variable not set",
            )
        })?
    } else if cfg!(target_os = "linux") {
        let home = env::var("HOME").map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, "HOME environment variable not set")
        })?;
        let mut path = PathBuf::from(home);
        path.push(".local");
        path.push("share");
        path
    } else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unsupported operating system",
        ));
    };

    let path = base_dir.join("life_monitor");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}
