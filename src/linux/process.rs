use crate::{
    linux::util::{get_focused_window, get_idle_time},
    localdb::*,
    processinfo::ProcessInfo,
};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use sysinfo::System;
use tokio::{
    sync::mpsc,
    time::{interval, Duration},
};
use tracing::*;

static TRACKER: Lazy<Mutex<ProcessTracker>> = Lazy::new(|| Mutex::new(ProcessTracker::new()));

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

        let d = get_process_data(&con).unwrap_or_else(|err| {
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

#[derive(Copy, Clone)]
enum Event {
    Tick,
    IdleCheck,
    DbUpdate,
}

pub async fn init() {
    debug!("Process task spawned!");

    // I should reuse the connection from new somehow but in that way is simple and i think the overload should not
    // be to great since it's only at the start.
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
    let (tx, mut rx) = mpsc::channel(100);
    spawn_ticker(tx.clone(), Duration::from_secs(1), Event::Tick);
    spawn_ticker(tx.clone(), Duration::from_secs(10), Event::IdleCheck);
    spawn_ticker(tx.clone(), Duration::from_secs(300), Event::DbUpdate);

    let mut idle = false;

    while let Some(event) = rx.recv().await {
        let mut tracker = TRACKER.lock().expect("poisoned");

        match event {
            Event::Tick => {
                if !idle {
                    handle_active_window(&mut tracker).await;
                }
            }
            Event::IdleCheck => {
                idle = check_idle(&tracker);
            }
            Event::DbUpdate => {
                if let Err(e) = send_to_process_table(&con, &tracker.procs) {
                    error!("Error sending data to time_wasted table. Error: {e:?}");
                }
            }
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

async fn handle_active_window(tracker: &mut ProcessTracker) {
    // get_focused_window returns a error if there is no window focused, like when you are in the
    // workspace.
    //
    // Below i tried to reduce the overload by only updating the time of the proc of the active
    // window only when the window have changed, don't know how much this worth is though.
    //
    // The time in the window focused in calculate using the difference in the system time between
    // the function call.
    match get_focused_window() {
        Ok((name, instance, class)) => {
            let uptime = System::uptime();

            // if last_window_name is emtpy we are in the first window, without this the program
            // update time in the wrong order.
            if !tracker.last_window_name.is_empty() && tracker.last_window_name != class {
                let time_diff = uptime - tracker.time;
                tracker.time = 0;

                update_time_for_app(&mut tracker.procs, &name, time_diff, instance, &class);
            }

            if tracker.time == 0 {
                tracker.time = uptime;
                tracker.last_window_name = class;
            }
        }

        Err(_) => {}
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

fn update_time_for_app(
    tracking_data: &mut Vec<ProcessInfo>,
    app_name: &str,
    time: u64,
    instance: String,
    window_class: &str,
) {
    if let Some(info) = tracking_data.iter_mut().find(|p| p.name == app_name) {
        info.time_spent += time;
        info.instance = instance;
        info.window_class = window_class.to_string();
    } else {
        tracking_data.push(ProcessInfo {
            name: app_name.to_string(),
            time_spent: time,
            instance,
            window_class: window_class.to_string(),
        });
    }
}
