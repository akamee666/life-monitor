use crate::{
    linux::util::{get_focused_window, get_idle_time},
    localdb::*,
    processinfo::ProcessInfo,
};
use rusqlite::Connection;
use std::sync::Arc;
use sysinfo::System;
use tokio::{
    sync::mpsc,
    sync::Mutex,
    time::{interval, Duration},
};
use tracing::*;

#[derive(Debug)]
pub struct ProcessTracker {
    time: u64,
    last_window_name: String,
    idle_period: u64,
    procs: Vec<ProcessInfo>,
}

impl ProcessTracker {
    fn new(con: &Connection) -> Self {
        let d = get_proct(con).unwrap_or_else(|err| {
            error!(
                "Connection with the proc table was opened but could not receive data from table, quitting!\n Err: {:?}",
                err
            );
            panic!("{err}");
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
    let con = open_con().unwrap_or_else(|err| {
        error!(
            "Could not open a connection with local database for Procs, quitting!\n Err: {:?}",
            err
        );
        panic!();
    });

    let db_int = if let Some(interval) = interval {
        info!("Interval argument provided, changing values.");
        interval
    } else {
        300
    };

    let logger = Arc::new(Mutex::new(ProcessTracker::new(&con)));

    let (tx, mut rx) = mpsc::channel(200);

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
                idle = check_idle(&tracker);
            }
            Event::DbUpdate => {
                let tracker = logger.lock().await;
                debug!("Database event tick, sending data from procs now.");
                //debug!("Current logger data from proc: {:?}", tracker);
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

fn check_idle(tracker: &ProcessTracker) -> bool {
    let duration = get_idle_time().unwrap().as_secs();
    if duration > tracker.idle_period {
        debug!("Info is currently idle, we should stop tracking!");
        true
    } else {
        false
    }
}

// Below i tried to reduce the overload by only updating the time of the proc of the active
// window only when the window have changed, don't know how much this worth is though.
// The time in the window focused in calculate using the difference in the system time between
// function calls.
async fn handle_active_window(tracker: &mut ProcessTracker) {
    if let Ok((name, instance, class)) = get_focused_window() {
        //debug!(
        //    "Window name:[{}], Window instance:[{}], Window class:[{}]",
        //    name, instance, class
        //);

        let uptime = System::uptime();

        // if last_window_name is emtpy we are in the first window, without this the program
        // update time in the wrong order.
        // So if we are not in the first window and the currently window is different than before,
        // we check the time and update our vector.
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

// THIS FUNCTION IS A ABSOLUTE MESS.
fn update_time_for_app(
    tracking_data: &mut Vec<ProcessInfo>,
    window_name: String,
    instance: String,
    window_class: &String,
    time: u64,
) {
    // First we find the window we want to update the time.
    if let Some(info) = tracking_data
        .iter_mut()
        .find(|p| p.instance == instance && p.window_class == *window_class)
    {
        // Update existing entry.
        info.time_spent += time;

        if info.name != window_name {
            debug!(
                "Different name when updating window, info.name: {}. window_name: {}",
                info.name, window_name
            );
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
