pub fn record_window_time(procs: &mut Vec<ProcessInfo>, window: Window, time_actived: Duration) {
    let elapsed_secs = time_actived.as_secs();

    // Don't record empty durations
    if elapsed_secs == 0 {
        return;
    }

    debug!(
        "Recording {} seconds for window {:?}",
        elapsed_secs, window.w_class
    );

    // Find the existing process entry or create a new one
    if let Some(proc) = procs.iter_mut().find(|p| p.w_name == window.w_name) {
        proc.w_time += elapsed_secs;
    } else {
        procs.push(ProcessInfo {
            w_name: window.w_name,
            w_class: window.w_class,
            w_time: elapsed_secs,
        });
    }
}
