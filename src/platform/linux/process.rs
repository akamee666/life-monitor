use crate::common::*;
use crate::keylogger::is_idle;
use crate::storage::backend::{DataStore, StorageBackend};

use crate::platform::linux::wayland::*;

use anyhow::*;
use tokio::sync::mpsc::*;
use tokio::time::*;

use tracing::*;

#[cfg(feature = "x11")]
pub async fn run_x11(update_interval: u32, backend: StorageBackend) -> Result<()> {
    let mut x11_ctx = X11Ctx::new()?;

    // spawn active window ticker, since x11 is not event driven, we need to be checking the active
    // window every second
    let ticker = spawn_ticker(tasks_tx.clone(), Duration::from_secs(1), Signals::Tick);
    tokio::spawn(ticker);

    loop {
        tokio::select! {
            Some(signal) = tasks_rx.recv() => match signal {
                TaskSignals::Tick => {
                    if !is_idle() {
                        if let Some(x11) = &mut x11_ctx {
                            handle_active_window(&mut processes_data, x11).await?;
                        }
                    }
                }
                TaskSignals::DbUpdate => {
                    debug!("Sending procs: {:#?}", processes_data.procs);
                    if let Err(err) = backend.store_proc_data(&processes_data.procs).await {
                        error!("Error sending data to procs table: {err:?}");
                    }
                }
            }

        }
    }
}

pub async fn run_wayland(
    mut proc_data: ProcessTracker,
    update_interval: u32,
    backend: StorageBackend,
) -> Result<()> {
    let (events_tx, mut events_rx) = channel::<FocusEvent>(240);

    // spawn Wayland listener
    tokio::task::spawn_blocking(move || {
        if let Err(e) = listen_for_wlevents(events_tx) {
            error!("Wayland listener failed: {:?}", e);
        }
    });

    let mut current: Option<(Window, Instant)> = None;
    let mut idle_check = interval(Duration::from_secs(20));
    let mut database_update = interval(Duration::from_secs(update_interval as u64));

    loop {
        tokio::select! {
            Some(event) = events_rx.recv() => match event {
                FocusEvent::FocusGained(new_window) => {
                    if let Some((old_window, start_time)) = current.take() {
                        record_window_time(&mut proc_data.procs, old_window, start_time.elapsed());
                    }
                    current = Some((new_window, Instant::now()));
                }
                FocusEvent::FocusLost(lost_window) => {
                    if let Some((old_window, start_time)) = current.take() {
                        if old_window.w_class == lost_window.w_class {
                            record_window_time(&mut proc_data.procs, old_window, start_time.elapsed());
                        }
                    }
                }
            },

            // Idle check
            _ = idle_check.tick() => {
                if let Some((window, _)) = &current {
                    if is_idle() {
                        info!("User idle, pausing timer for {:?}", window.w_class);
                        let (idle_window, idle_start_time) = current.take().unwrap();
                        record_window_time(&mut proc_data.procs, idle_window, idle_start_time.elapsed());
                    }
                }
            }

            _ = database_update.tick() => {
                    debug!("Sending procs: {:#?}", proc_data.procs);
                    if let Err(err) = backend.store_proc_data(&proc_data.procs).await {
                        error!("Error sending data to procs table: {err:?}");
                }
            }

        }
    }
}

pub async fn run(update_interval: u32, backend: StorageBackend) -> Result<()> {
    let proc_data = ProcessTracker::new(&backend).await?;
    let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();

    if is_wayland {
        run_wayland(proc_data, update_interval, backend).await?;
    } else {
        #[cfg(feature = "x11")]
        run_x11(proc_data).await;

        #[cfg(not(feature = "x11"))]
        {
            error!("Running under X11 but binary was built without `x11` feature, rebuild it with: `cargo build --features x11`");
            return Err(anyhow!("X11 feature not enabled"));
        }
    }

    anyhow::bail!("This should be unreachable");
}
