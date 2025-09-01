#[cfg(target_os = "linux")]
pub mod linux;
use crate::platform::linux::util::get_focused_window;

#[cfg(target_os = "windows")]
pub mod win;

#[cfg(target_os = "windows")]
use crate::platform::win::util::get_focused_window;

use crate::update_window_time;
use crate::ProcessTracker;

use sysinfo::System;

use tracing::*;

// This function upload the time for the entry in the vector only if we change window to reduce the
// overload by not going through the vector every second.
pub async fn handle_active_window(tracker: &mut ProcessTracker) {
    if let Ok((w_name, w_class)) = get_focused_window() {
        let uptime = System::uptime();

        if tracker.last_wname != w_name {
            if !tracker.last_wname.is_empty() {
                debug!(
                    "We are not in the same window than before. Going to update time for last window {}.",
                    tracker.last_wclass
                );

                let time_diff = uptime - tracker.time;

                debug!(
                    "Uptime for new window is not zero, window: {} was active for: [{}] seconds.",
                    tracker.last_wclass, time_diff
                );

                // The window that will be updated will be last but we need to reset the timer here
                // for the new window.
                tracker.time = 0;

                update_window_time(
                    &mut tracker.procs,
                    tracker.last_wname.clone(),
                    tracker.last_wclass.clone(),
                    time_diff,
                );
            } else {
                debug!("Last window is empty, we just start the program.");
                debug!("Going to add the currently window as first entry.");
                update_window_time(&mut tracker.procs, w_name.clone(), w_class.clone(), 0);
            }
        } else {
            debug!("We are in the same window than before, doing nothing.");
            debug!("Timer: {}s", uptime - tracker.time);
        }

        // Timer will be zero if the program just started or windows have changed and we just
        // finished updating the vector.
        if tracker.time == 0 {
            debug!("Timer is zero, recording uptime now to have the difference later.");
            tracker.time = uptime;
            tracker.last_wname = w_name;
            tracker.last_wclass = w_class;
        }
    };
}
