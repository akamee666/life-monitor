use crate::backend::{DataStore, StorageBackend};
use crate::is_idle;
use crate::platform::handle_active_window;
use crate::spawn_ticker;
use crate::Event;
use crate::ProcessTracker;

use std::sync::Arc;

use tokio::sync::mpsc::channel;
use tokio::sync::Mutex;
use tokio::time::Duration;

use tracing::*;

pub async fn init(interval: Option<u32>, backend: StorageBackend) {
    // Adding some difference between the two tasks so they don't try to call database at the
    // same time and waste time waiting for lock.
    let db_int = if let Some(interval) = interval {
        info!("Interval argument provided, changing values.");
        interval + 5
    } else {
        300 + 5
    };

    let logger = Arc::new(Mutex::new(ProcessTracker::new(&backend).await));
    let (tx, mut rx) = channel(20);

    // Each one second we gonna report the current window
    spawn_ticker(tx.clone(), Duration::from_secs(1), Event::Tick);
    // Each twenty seconds we gonna check if user is idle
    spawn_ticker(tx.clone(), Duration::from_secs(20), Event::IdleCheck);
    // Each [interval here] seconds we gonna send updates to the database
    spawn_ticker(
        tx.clone(),
        Duration::from_secs(db_int.into()),
        Event::DbUpdate,
    );

    let mut idle = false;
    while let Some(event) = rx.recv().await {
        match event {
            Event::Tick => {
                if !idle {
                    let mut tracker = logger.lock().await;
                    handle_active_window(&mut tracker).await;
                }
            }
            Event::IdleCheck => {
                let tracker = logger.lock().await;
                idle = is_idle(&tracker.idle_period);
            }
            Event::DbUpdate => {
                let tracker = logger.lock().await;
                if let Err(e) = backend.store_proc_data(&tracker.procs).await {
                    error!("Error sending data to procs table. Error: {e}");
                }
            }
        }
    }
}
