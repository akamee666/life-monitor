use crate::data::{DataStore, StorageBackend};
use crate::linux::util::get_focused_window;
use crate::spawn_ticker;
use crate::Event;
use crate::ProcessTracker;
use crate::{check_idle, update_window_time};

use sysinfo::System;

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
                idle = check_idle(&tracker.idle_period);
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

// Below i tried to reduce the overload by only updating the time of the proc of the active
// window only when the window have changed, don't know how much this worth is though.
// The time in the window focused in calculate using the difference in the system time between
// function calls.
async fn handle_active_window(tracker: &mut ProcessTracker) {
    if let Ok((name, instance, class)) = get_focused_window() {
        debug!(
            "Window name:{}. Window instance:{}. Window class:{}.",
            name, instance, class
        );

        let uptime = System::uptime();

        // if last_window_class is emtpy we are in the first window, without this the program
        // update time in the wrong order.
        // So if we are not in the first window and the currently window is different than before,
        // we check the time and update our vector.
        if !tracker.last_window_class.is_empty() && tracker.last_window_class != class {
            let time_diff = uptime - tracker.time;
            tracker.time = 0;

            update_window_time(&mut tracker.procs, name, class.clone(), instance, time_diff);
        }

        if tracker.time == 0 {
            tracker.time = uptime;
            tracker.last_window_class = class;
        }
    };
}
