use crate::common::*;
use tokio::time::Duration;
use tracing::*;

#[derive(Debug, Clone)]
pub struct Window {
    pub name: String,
    pub class: String,
}

// TODO: the fuck do i need to do here? This needs to change, window should be a generic that can handle Window, or? i don't know
pub fn record_window_time(procs: &mut Vec<ProcessInfo>, window: Window, time_actived: Duration) {
    let elapsed_secs = time_actived.as_secs();

    // Don't record empty durations
    if elapsed_secs == 0 {
        return;
    }

    debug!(
        "Recording {} seconds for window {:?}",
        elapsed_secs, window.name
    );

    // Find the existing process entry or create a new one
    if let Some(proc) = procs.iter_mut().find(|p| p.w_name == window.name) {
        proc.w_time += elapsed_secs;
    } else {
        procs.push(ProcessInfo {
            w_name: window.name,
            w_class: window.class,
            w_time: elapsed_secs,
        });
    }
}
