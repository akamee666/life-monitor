use crate::{common::*, platform::windows::common::*, storage::backend::*};

use anyhow::{Context, Result};
use tokio::time::*;
use tracing::*;

pub async fn run(update_interval: u32, backend: StorageBackend) -> Result<()> {
    let mut procs_data = ProcessTracker::new(&backend).await?;

    let mut tick = interval(Duration::from_secs(1));
    let mut database_update = interval(Duration::from_secs(update_interval as u64));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                if !is_idle() {
                    let (w_name, w_class) = get_focused_window().with_context(|| "Failed to find foreground window")?;
                    let now = uptime();

                    if procs_data.last_wname.is_empty() {
                        debug!("First run, recording initial window: '{w_name}'");
                        procs_data.procs.push(ProcessInfo {
                            w_name: w_name.clone(),
                            w_time: 0,
                            w_class: w_class.clone(),
                        });
                        procs_data.last_wname = w_name;
                        procs_data.last_wclass = w_class;
                        procs_data.time = now;
                    } else if procs_data.last_wname != w_name {
                        let elapsed = now - procs_data.time;
                        debug!(
                            "Focus changed, Window: '{}' was active for: {}s",
                            procs_data.last_wclass, elapsed
                        );
                        debug!("Starting counting time for the new window: '{w_name}'");

                        // Update time for the *previous* window
                        if let Some(prev) = procs_data
                            .procs
                                .iter_mut()
                                .find(|p| p.w_name == procs_data.last_wname)
                        {
                            prev.w_time += elapsed;
                        } else {
                            procs_data.procs.push(ProcessInfo {
                                w_name: procs_data.last_wname.clone(),
                                w_time: elapsed,
                                w_class: procs_data.last_wclass.clone(),
                            });
                        }

                        // Record the new window
                        if procs_data.procs.iter().all(|p| p.w_name != w_name) {
                            procs_data.procs.push(ProcessInfo {
                                w_name: w_name.clone(),
                                w_time: 0,
                                w_class: w_class.clone(),
                            });
                        }

                        procs_data.last_wname = w_name;
                        procs_data.last_wclass = w_class;
                        procs_data.time = now;

                    }
                }
            }

            _ = database_update.tick() => {
                if let Err(err) = backend.store_proc_data(&procs_data.procs).await {
                    error!("Error sending data to procs table: {err:?}");
                }

            }
        }
    }
}
