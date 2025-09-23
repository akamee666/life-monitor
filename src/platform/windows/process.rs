use crate::common::*;
use crate::platform::windows::common::*;
use crate::storage::backend::{DataStore, StorageBackend};

use anyhow::{Context, Result};
use tokio::time::*;
use tracing::*;

pub async fn run(update_interval: u32, backend: StorageBackend) -> Result<()> {
    let proc_data = ProcessTracker::new(&backend).await?;

    let mut tick = interval(Duration::from_secs(1));
    let mut database_update = interval(Duration::from_secs(update_interval as u64));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                if !is_idle() {
                    // TODO: IMPLEMENT CODE
                    let (w_name, w_class) = get_focused_window().with_context(|| "Failed to find foreground window")?;
                }
            }

            _ = database_update.tick() => {
                if let Err(err) = backend.store_proc_data(&proc_data.procs).await {
                    error!("Error sending data to procs table: {err:?}");
                }

            }
        }
    }
}
