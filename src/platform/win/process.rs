use crate::data::DataStore;
use crate::spawn_ticker;
use crate::win::util::get_focused_window;
use crate::Event;
use crate::ProcessTracker;
use crate::StorageBackend;
use crate::{check_idle, update_w_time};

use std::sync::Arc;
use tokio::sync::mpsc::channel;
use tokio::sync::Mutex;
use tokio::time::Duration;

use sysinfo::System;

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

// This function upload the time for the entry in the vector only if we change window to reduce the
// overload by not going through the vector every second.
async fn handle_active_window(tracker: &mut ProcessTracker) {
    if let Ok((w_name, w_class)) = get_focused_window() {
        println!();
        debug!("Window name: {}.", w_name);
        debug!("Window class: {}.", w_class);
        debug!("Window instance: {}.", w_instance);
        println!();

        let uptime = System::uptime();

        if tracker.last_wname != w_name {
            if !tracker.last_wname.is_empty() {
                debug!(
                    "We are not in the same window than before. Going to update time for last window, currently Vec is: {:#?}",
                    tracker.procs
                );

                let time_diff = uptime - tracker.time;

                debug!(
                    "Uptime for new window is not zero, window was active for: [{}] seconds.",
                    time_diff
                );

                // The window that will be updated will be last but we need to reset the timer here
                // for the new window.
                tracker.time = 0;

                update_w_time(
                    &mut tracker.procs,
                    tracker.last_wname.clone(),
                    tracker.last_wclass.clone(),
                    tracker.last_winstance.clone(),
                    time_diff,
                );
            } else {
                debug!("Last window is empty, we just start the program.");
                debug!("Going to add the currently window as first entry.");
                update_w_time(
                    &mut tracker.procs,
                    w_name.clone(),
                    w_class.clone(),
                    w_instance.clone(),
                    0,
                );
            }
        } else {
            debug!("We are in the same window than before, doing nothing.");
            debug!("Time difference: [{}]", uptime - tracker.time);
        }

        // Timer will be zero if the program just started or windows have changed and we just
        // finished updating the vector.
        if tracker.time == 0 {
            debug!("Timer is zero, recording uptime now to have the difference later.");
            tracker.time = uptime;
            tracker.last_wname = w_name;
            tracker.last_winstance = w_instance;
            tracker.last_wclass = w_class;
        }
    };
}
