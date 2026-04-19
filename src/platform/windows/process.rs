use crate::{common::*, platform::windows::common::*, storage::backend::*};

use anyhow::{Context, Result};
use tokio::time::*;
use tracing::*;

fn read_focused_window(idle: bool) -> Result<Option<Window>> {
    if idle {
        Ok(None)
    } else {
        get_focused_window()
            .map(|(name, class)| Some(Window { name, class }))
            .with_context(|| "Failed to find foreground window")
    }
}

fn update_focus_tracker(
    tracker: &mut ProcessTracker,
    now: chrono::DateTime<chrono::Utc>,
    idle: bool,
    focused_window: Result<Option<Window>>,
) {
    if idle {
        sync_focus_tracker(tracker, None, now, true);
        return;
    }

    match focused_window {
        Ok(window) => sync_focus_tracker(tracker, window, now, false),
        Err(err) => {
            warn!("Failed to read foreground window on Windows: {err:#}");
            sync_focus_tracker(tracker, None, now, false);
        }
    }
}

pub async fn run(update_interval: u32, backend: StorageBackend) -> Result<()> {
    let mut procs_data =
        ProcessTracker::new(backend.source_id(), backend.bucket_granularity_minutes());

    let mut tick = interval(Duration::from_secs(1));
    let mut database_update = interval(Duration::from_secs(update_interval as u64));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let now = chrono::Utc::now();
                let idle = is_idle();
                let focused_window = read_focused_window(idle);
                update_focus_tracker(&mut procs_data, now, idle, focused_window);
            }

            _ = database_update.tick() => {
                procs_data.record_active_until(chrono::Utc::now());
                let rows = procs_data.drain_pending();
                if let Err(err) = backend.store_proc_data(&rows).await {
                    error!("Error sending data to procs table: {err:?}");
                }

            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{DEFAULT_BUCKET_MINUTES, DEFAULT_SOURCE_ID};
    use chrono::{TimeZone, Utc};

    #[test]
    fn update_focus_tracker_clears_focus_after_lookup_error() {
        let mut tracker = ProcessTracker::new(DEFAULT_SOURCE_ID, DEFAULT_BUCKET_MINUTES as u32);
        let start = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap();
        let error_at = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 15).unwrap();
        let flush_at = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 30).unwrap();

        update_focus_tracker(
            &mut tracker,
            start,
            false,
            Ok(Some(Window {
                name: "Editor".to_string(),
                class: "nvim.exe".to_string(),
            })),
        );
        update_focus_tracker(
            &mut tracker,
            error_at,
            false,
            Err(anyhow::anyhow!("foreground window disappeared")),
        );
        tracker.record_active_until(flush_at);

        let rows = tracker.drain_pending();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].focus_seconds, 15);
        assert_eq!(rows[0].window_class, "nvim.exe");
    }

    #[test]
    fn update_focus_tracker_ignores_lookup_error_while_idle() {
        let mut tracker = ProcessTracker::new(DEFAULT_SOURCE_ID, DEFAULT_BUCKET_MINUTES as u32);
        let start = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap();
        let idle_at = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 15).unwrap();
        let resume_at = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 30).unwrap();

        update_focus_tracker(
            &mut tracker,
            start,
            false,
            Ok(Some(Window {
                name: "Docs".to_string(),
                class: "firefox.exe".to_string(),
            })),
        );
        update_focus_tracker(
            &mut tracker,
            idle_at,
            true,
            Err(anyhow::anyhow!("idle windows lookup should be skipped")),
        );
        update_focus_tracker(
            &mut tracker,
            resume_at,
            false,
            Ok(Some(Window {
                name: "Docs".to_string(),
                class: "firefox.exe".to_string(),
            })),
        );
        tracker.record_active_until(Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 45).unwrap());

        let rows = tracker.drain_pending();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].window_class, "firefox.exe");
        assert_eq!(rows[0].focus_seconds, 30);
    }
}
