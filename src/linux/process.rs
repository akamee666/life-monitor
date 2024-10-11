use crate::{
    linux::util::{get_focused_window, get_idle_time},
    localdb::*,
    processinfo::ProcessInfo,
};
use once_cell::sync::Lazy;
use std::sync::Arc;
use sysinfo::System;
use tokio::{
    sync::mpsc,
    sync::RwLock,
    time::{interval, Duration},
};
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

impl ProcessTracker {
    fn new() -> Self {
        // Get values stored in database, open_con already check if there is a database to get data
        // from.
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

        let d = get_proct(&con).unwrap_or_else(|err| {
            debug!(
                "Could not open a connection with local database, quitting! Err: {:?}",
                err
            );
            panic!(
                "Could not open a connection with local database, quitting! Err: {:?}",
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

#[derive(Copy, Clone, Debug)]
enum Event {
    Tick,
    IdleCheck,
    DbUpdate,
}

pub async fn init(interval: Option<u32>) {
    debug!("WindowTracker spawned.");
    debug!("Opening connection for window tracker.");

    let mut db_interval = 300;
    if interval.is_some() {
        debug!("Interval argument provided, changing values.");
        db_interval = interval.unwrap();
    }

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

    // Create a channel and spawn tasks that needs to run at certain periods.
    // I really don't know if it's ok to clone the sender, but it's work so i'll let it.
    debug!("Creating channels for events.");
    let (tx, mut rx) = mpsc::channel(100);
    spawn_ticker(tx.clone(), Duration::from_secs(1), Event::Tick);
    spawn_ticker(tx.clone(), Duration::from_secs(20), Event::IdleCheck);
    spawn_ticker(
        tx.clone(),
        Duration::from_secs(db_interval.into()),
        Event::DbUpdate,
    );

    let mut idle = false;
    while let Some(event) = rx.recv().await {
        match event {
            Event::Tick => {
                let mut tracker = TRACKER.write().await;
                if !idle {
                    handle_active_window(&mut tracker).await;
                }
            }
            Event::IdleCheck => {
                let tracker = TRACKER.read().await;
                idle = check_idle(&tracker);
            }
            Event::DbUpdate => {
                let tracker = TRACKER.read().await;
                if let Err(e) = update_proct(&con, &tracker.procs) {
                    error!("Error sending data to time_wasted table. Error: {e:?}");
                }
            }
        }
    }
}

fn spawn_ticker(tx: mpsc::Sender<Event>, duration: Duration, event: Event) {
    debug!("Spawning ticker: {:?}", event);
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

// get_focused_window returns a error if there is no window focused, like when you are in the
// workspace.
//
// Below i tried to reduce the overload by only updating the time of the proc of the active
// window only when the window have changed, don't know how much this worth is though.
//
// The time in the window focused in calculate using the difference in the system time between
// the function call.
async fn handle_active_window(tracker: &mut ProcessTracker) {
    debug!("Handle tick received! Handling active window");
    if let Ok((name, instance, class)) = get_focused_window() {
        // Uncomment this for more detailed info about the window.
        //debug!(
        //    "Window name:[{}], Window instance:[{}], Window class:[{}]",
        //    name, instance, class
        //);
        debug!("Window class:[{}]", class);

        let uptime = System::uptime();

        // if last_window_name is emtpy we are in the first window, without this the program
        // update time in the wrong order.
        if !tracker.last_window_name.is_empty() && tracker.last_window_name != class {
            let time_diff = uptime - tracker.time;
            tracker.time = 0;

            update_time_for_app(&mut tracker.procs, name, instance, &class, time_diff);
        }

        if tracker.time == 0 {
            tracker.time = uptime;
            tracker.last_window_name = class;
        }
    };
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

// TODO: This function is confusing, i should doc it better cause i do not think there is a better
// way to write this code.
fn update_time_for_app(
    tracking_data: &mut Vec<ProcessInfo>,
    window_name: String,
    instance: String,
    window_class: &String,
    time: u64,
) {
    debug!(
        "We have a different window, updating time from: [{}]",
        window_class
    );
    if let Some(info) = tracking_data
        .iter_mut()
        .find(|p| p.instance == instance && p.window_class == *window_class)
    {
        // Update existing entry.
        info.time_spent += time;
        // Update name if it's different.
        if info.name != window_name {
            info.name = window_name;
        }
    } else {
        // Add new entry
        tracking_data.push(ProcessInfo {
            name: window_name,
            time_spent: time,
            instance,
            window_class: window_class.to_string(),
        });
    }
}
