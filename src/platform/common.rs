//! This file is responsible to store functions, enums or
//! structs that can be used for all platforms supported.
use crate::common::*;
use tracing::*;

#[cfg(target_os = "linux")]
use crate::platform::linux::x11::get_idle_time;

#[cfg(target_os = "windows")]
use crate::platform::windows::common::*;

#[cfg(target_os = "windows")]
use windows::Win32::System::SystemInformation::GetTickCount64;

#[cfg(target_os = "linux")]
use crate::platform::linux::x11::get_focused_window;

// This function upload the time for the entry in the vector only if we change window to reduce the
// overload by not going through the vector every second.
pub async fn handle_active_window(tracker: &mut ProcessTracker) {
    if let Ok((w_name, w_class)) = get_focused_window() {
        let uptime = uptime();
        // We check by name, two differents windows of the same program doing completely unrelated things will have the same class, we want the data more fine grained.
        if tracker.last_wname != w_name {
            // We also need to check if the last_window is not empty to cover the first call of the program
            if !tracker.last_wname.is_empty() {
                let time_diff = uptime - tracker.time;

                debug!(
                    "We are not in the same window than before. Going to update time for last window '{}'.",
                    tracker.last_wclass
                );
                debug!(
                    "Uptime for new window is not zero, window: '{}' was active for: [{}] seconds.",
                    tracker.last_wclass, time_diff
                );
                debug!("Starting counting time for the new window: '{w_name}'");

                // The window that is going to be updated in the struct is the one just lost focus (last window). Our timer needs to be reset
                // so we can start couting the time for the new window that is now in focus.
                tracker.time = 0;

                if let Some(existent_window) = tracker.procs.iter_mut().find(|p| p.w_name == w_name)
                {
                    debug!("Updating time for existent vector rather than adding new entry.");
                    existent_window.w_time += time_diff;
                } else {
                    debug!("Window is new, adding the new entry");
                    tracker.procs.push(ProcessInfo {
                        w_name: tracker.last_wname.to_owned(),
                        w_time: time_diff,
                        w_class: tracker.last_wclass.to_owned(),
                    });
                }
            } else {
                debug!("Last window is empty, we just start the program. Going to add the currently window as first entry.");
                tracker.procs.push(ProcessInfo {
                    w_name: w_name.clone(),
                    w_time: 0,
                    w_class: w_class.clone(),
                });
            }
        }
        // Timer will be zero if the program just started or windows have changed and we just
        // finished updating the vector.
        if tracker.time == 0 {
            debug!("Timer is zero, recording uptime now to have the difference later.");
            tracker.time = uptime;
            tracker.last_wname = w_name;
            tracker.last_wclass = w_class;
        }
    } else {
        // We don't need to quit just because we failed to find the active window.
        error!("Failed to find the active window");
    };
}

#[cfg(target_os = "linux")]
fn uptime() -> u64 {
    let content = fs::read_to_string("/proc/uptime").unwrap_or_else(|err| {
        error!("Failed to read /proc/uptime: {}", err);
        panic!("Cannot continue execution");
    });

    content
        .split_whitespace()
        .next()
        .unwrap_or_else(|| {
            error!("Unexpected /proc/uptime format");
            panic!("Cannot continue execution");
        })
        .split('.')
        .next()
        .unwrap()
        .parse()
        .unwrap_or_else(|err| {
            error!("Failed to parse uptime: {}", err);
            panic!("Cannot continue execution");
        })
}

#[cfg(target_os = "windows")]
fn uptime() -> u64 {
    unsafe { GetTickCount64() / 1_000 }
}

// This function uses get_idle_time which is imported from win module or linux mod.
// I wonder if it's better to just one function and use cfg!(target_os = "x") instead of two
// different functions, each of them inside of its own platform module.
pub fn is_idle(idle_period: &u64) -> bool {
    debug!("Checking if user is idle");
    let duration = get_idle_time().unwrap().as_secs();
    if duration > *idle_period {
        debug!("User is idle, stopping!");
        true
    } else {
        debug!("User is not idle.");
        false
    }
}
