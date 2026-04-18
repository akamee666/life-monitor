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

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that zero-second durations are ignored so focus changes do not create
    /// empty process rows during fast window transitions.
    #[test]
    fn record_window_time_ignores_zero_duration() {
        let mut procs = Vec::new();

        record_window_time(
            &mut procs,
            Window {
                name: "Editor".to_string(),
                class: "nvim".to_string(),
            },
            Duration::from_secs(0),
        );

        assert!(procs.is_empty());
    }

    /// Verifies that repeated focus time for the same window accumulates into one process
    /// entry instead of creating duplicates.
    #[test]
    fn record_window_time_accumulates_existing_window() {
        let mut procs = vec![ProcessInfo {
            w_name: "Editor".to_string(),
            w_time: 5,
            w_class: "nvim".to_string(),
        }];

        record_window_time(
            &mut procs,
            Window {
                name: "Editor".to_string(),
                class: "nvim".to_string(),
            },
            Duration::from_secs(7),
        );

        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].w_time, 12);
    }

    /// Verifies that a new process row is created when a previously unseen window becomes
    /// active for a measurable amount of time.
    #[test]
    fn record_window_time_inserts_new_window() {
        let mut procs = Vec::new();

        record_window_time(
            &mut procs,
            Window {
                name: "Browser".to_string(),
                class: "firefox".to_string(),
            },
            Duration::from_secs(9),
        );

        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].w_name, "Browser");
        assert_eq!(procs[0].w_class, "firefox");
        assert_eq!(procs[0].w_time, 9);
    }
}
