use crate::common::*;
use crate::platform::common::*;
use crate::platform::linux::inputs::*;
use crate::platform::linux::wayland::*;
use crate::storage::backend::{DataStore, StorageBackend};

use anyhow::*;
use tokio::sync::mpsc::*;
use tokio::time::*;

use tracing::*;

#[cfg(feature = "x11")]
pub async fn run_x11(
    mut proc_data: ProcessTracker,
    update_interval: u32,
    backend: StorageBackend,
) -> Result<()> {
    use crate::platform::linux::x11::*;

    let x11_ctx = X11Ctx::new()?;

    let mut tick = interval(Duration::from_secs(1));
    let mut database_update = interval(Duration::from_secs(update_interval as u64));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                // is_idle should be under common.rs since it can be used no matter if user is x11 or wayland
                if !is_idle() {
                    handle_active_window(&x11_ctx, &mut proc_data).await?;
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

enum TrackingState {
    /// No window is being tracked at the moment
    NoFocus,
    /// A window is focused and the user isn't idle
    Active(Window, Instant),
    /// A window is focused, but the user is idle (timer now is paused)
    Idle(Window),
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

    let mut state = TrackingState::NoFocus;
    let mut idle_check = interval(Duration::from_secs(20));
    let mut database_update = interval(Duration::from_secs(update_interval as u64));

    loop {
        tokio::select! {
            Some(event) = events_rx.recv() => match event {
                FocusEvent::FocusGained(new_window) => {
                    // if a previous window was active, record its time before switching
                    if let TrackingState::Active(old_window, start_time) = state {
                        record_window_time(&mut proc_data.procs, old_window, start_time.elapsed());
                    }
                    // set the new window as being active to start its time
                    state = TrackingState::Active(new_window, Instant::now());
                }
                FocusEvent::FocusLost(lost_window) => {
                    // we only care about this event if the window that lost focus
                    // is the one we are currently tracking as active.
                    if let TrackingState::Active(ref active_window, _) = state {
                        if active_window.name == lost_window.name {
                            // The currently tracked window is the one that lost focus.
                            // We take the state, record its time, and set the new state to NoFocus.
                            if let TrackingState::Active(old_window, start_time) = std::mem::replace(&mut state, TrackingState::NoFocus) {
                                record_window_time(&mut proc_data.procs, old_window, start_time.elapsed());
                            }
                        }
                        // if the windows do NOT match, we do nothing. This means a FocusGained
                        // event for another window has already occurred and the state is correct.
                    }
                }
            },

            _ = idle_check.tick() => {
                match state {
                    // the user was active, check if they've now become idle.
                    TrackingState::Active(ref window, start_time) => {
                        if is_idle() {
                            info!("User is now idle, pausing timer for {:?}", window.class);
                            // record the time accumulated before becoming idle.
                            record_window_time(&mut proc_data.procs, window.clone(), start_time.elapsed());
                            // transition to the Idle state, preserving the window info.
                            state = TrackingState::Idle(window.clone());
                        }
                    }
                    // the user was idle, check if they've now become active.
                    TrackingState::Idle(ref window) => {
                        if !is_idle() {
                            info!("User is active again, resuming timer for {:?}", window.class);
                            // Transition back to Active, restarting the timer from now.
                            state = TrackingState::Active(window.clone(), Instant::now());
                        }
                    }
                    // if no window is in focus, do nothing.
                    TrackingState::NoFocus => {}
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

pub async fn run(update_interval: u32, backend: StorageBackend) -> Result<()> {
    let proc_data = ProcessTracker::new(&backend).await?;
    let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();

    if is_wayland {
        run_wayland(proc_data, update_interval, backend).await?;
    } else {
        #[cfg(feature = "x11")]
        run_x11(proc_data, update_interval, backend).await?;

        #[cfg(not(feature = "x11"))]
        {
            error!("Running under X11 but binary was built without `x11` feature, rebuild it with: `cargo build --features x11`");
            return Err(anyhow!("X11 feature not enabled"));
        }
    }

    anyhow::bail!("This should be unreachable");
}
