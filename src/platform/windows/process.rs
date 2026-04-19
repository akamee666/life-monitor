use crate::{common::*, platform::windows::common::*, storage::backend::*};

use anyhow::{Context, Result};
use tokio::time::*;
use tracing::*;

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
                let focused_window = if idle {
                    None
                } else {
                    let (w_name, w_class) = get_focused_window().with_context(|| "Failed to find foreground window")?;
                    Some(Window { name: w_name, class: w_class })
                };

                sync_focus_tracker(&mut procs_data, focused_window, now, idle);
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
