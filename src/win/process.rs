use crate::{
    localdb::{get_proct, open_con, update_proct},
    processinfo::ProcessInfo,
    win::util::*,
};
use once_cell::sync::Lazy;
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};
use sysinfo::System;
use tokio::{sync::mpsc, time::interval};
use tracing::*;

static TRACKER: Lazy<Arc<RwLock<ProcessTracker>>> =
    Lazy::new(|| Arc::new(RwLock::new(ProcessTracker::new())));

#[derive(Debug)]
pub struct ProcessTracker {
    time: u64,
    last_window_name: String,
    idle_period: u64,
    procs: Vec<ProcessInfo>,
}

#[derive(Copy, Clone)]
enum Event {
    Tick,
    IdleCheck,
    DbUpdate,
}

impl ProcessTracker {
    fn new() -> Self {
        // Get values stored in database, open_con already check if there is a database to get data
        // from.
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

        let d = get_proct(&con).unwrap_or_else(|err| {
            error!(
                "Could get existing data from local database, quitting to not overwrite! Err: {:?}",
                err
            );
            panic!(
                "Could get existing data from local database, quitting to not overwrite! Err: {:?}",
                err
            );
        });

        ProcessTracker {
            time: 0,
            last_window_name: String::new(),
            idle_period: 20,
            procs: d,
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

fn check_idle(tracker: &ProcessTracker) -> bool {
    let duration = get_idle_time().unwrap().as_secs();
    if duration > tracker.idle_period {
        debug!("Info is currently idle, we should stop tracking!");
        true
    } else {
        false
    }
}

pub async fn init() {
    debug!("Process task spawned!");
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

    let (tx, mut rx) = mpsc::channel(100);
    spawn_ticker(tx.clone(), Duration::from_secs(1), Event::Tick);
    spawn_ticker(tx.clone(), Duration::from_secs(20), Event::IdleCheck);
    spawn_ticker(tx.clone(), Duration::from_secs(300), Event::DbUpdate);

    let mut idle = false;

    while let Some(event) = rx.recv().await {
        match event {
            Event::Tick => {
                let mut tracker = TRACKER.write().unwrap();
                if !idle {
                    handle_active_window(&mut tracker).await;
                }
            }
            Event::IdleCheck => {
                let tracker = TRACKER.read().expect("poisoned");
                idle = check_idle(&tracker);
            }
            Event::DbUpdate => {
                let tracker = TRACKER.read().expect("poisoned");
                if let Err(e) = update_proct(&con, &tracker.procs) {
                    error!("Error sending data to time_wasted table. Error: {e:?}");
                }
            }
        }
    }
}

async fn handle_active_window(tracker: &mut ProcessTracker) {
    // get_focused_window returns a error if there is no window focused, like when you are in the
    // workspace.
    //
    // Below i tried to reduce the overload by only updating the time of the proc of the active
    // window only when the window have changed, don't know how much this worth is though.
    //
    // The time in the window focused in calculate using the difference in the system time between
    // the function call.
    if let Ok((name, class)) = get_focused_window() {
        let uptime = System::uptime();

        // if last_window_name is emtpy we are in the first window, without this the program
        // update time in the wrong order.
        if !tracker.last_window_name.is_empty() && tracker.last_window_name != class {
            let time_diff = uptime - tracker.time;
            tracker.time = 0;

            update_time_for_app(&mut tracker.procs, &name, &class, time_diff);
        }

        if tracker.time == 0 {
            tracker.time = uptime;
            tracker.last_window_name = class;
        }
    };
}

fn update_time_for_app(
    tracking_data: &mut Vec<ProcessInfo>,
    window_name: &str,
    window_class: &str,
    time: u64,
) {
    let mut found = false;

    for info in tracking_data.iter_mut() {
        if info.name == window_name {
            info.time_spent += time;
            found = true;
            break;
        }
    }

    if !found {
        tracking_data.push(ProcessInfo {
            name: window_name.to_string(),
            time_spent: time,
            instance: window_class.to_string(), // Set instance to window_class
            window_class: window_class.to_string(),
        });
    }
}
