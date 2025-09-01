use crate::common::Event;
use crate::common::*;
use crate::platform::common::*;
use crate::storage::backend::{DataStore, StorageBackend};

use std::sync::Arc;
use tokio::sync::mpsc::channel;
use tokio::sync::Mutex;
use tokio::time::Duration;

use tracing::*;

pub async fn init(interval: Option<u32>, backend: StorageBackend) {
    let db_int = if let Some(interval) = interval {
        info!("Interval argument provided, changing values.");
        interval
    } else {
        300
    };

    let logger = Arc::new(Mutex::new(ProcessTracker::new(&backend).await));
    let (tx, mut rx) = channel(20);

    spawn_ticker(tx.clone(), Duration::from_secs(1), Event::Tick);
    spawn_ticker(tx.clone(), Duration::from_secs(20), Event::IdleCheck);
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
