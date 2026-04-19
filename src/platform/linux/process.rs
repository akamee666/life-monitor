use crate::common::*;
use crate::platform::linux::common::*;
use crate::platform::linux::inputs::*;
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
                } else {
                    proc_data.pause(chrono::Utc::now());
                }
            }

            _ = database_update.tick() => {
                proc_data.record_active_until(chrono::Utc::now());
                let rows = proc_data.drain_pending();
                if let Err(err) = backend.store_proc_data(&rows).await {
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
    Active(Window),
    /// A window is focused, but the user is idle (timer now is paused)
    Idle(Window),
}

#[cfg(feature = "wayland")]
pub async fn run_wayland(
    mut proc_data: ProcessTracker,
    update_interval: u32,
    backend: StorageBackend,
) -> Result<()> {
    use crate::platform::linux::wayland::*;

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
                    let now = chrono::Utc::now();
                    proc_data.switch_window(new_window.clone(), now);
                    // set the new window as being active to start its time
                    state = TrackingState::Active(new_window);
                }
                FocusEvent::FocusLost(lost_window) => {
                    // we only care about this event if the window that lost focus
                    // is the one we are currently tracking as active.
                    if let TrackingState::Active(ref active_window) = state {
                        if active_window.name == lost_window.name {
                            // The currently tracked window is the one that lost focus.
                            // We take the state, record its time, and set the new state to NoFocus.
                            proc_data.clear_focus(chrono::Utc::now());
                            state = TrackingState::NoFocus;
                        }
                        // if the windows do NOT match, we do nothing. This means a FocusGained
                        // event for another window has already occurred and the state is correct.
                    }
                }
            },

            _ = idle_check.tick() => {
                match state {
                    // the user was active, check if they've now become idle.
                    TrackingState::Active(ref window) => {
                        if is_idle() {
                            info!("User is now idle, pausing timer for {:?}", window.class);
                            proc_data.pause(chrono::Utc::now());
                            state = TrackingState::Idle(window.clone());
                        }
                    }
                    // the user was idle, check if they've now become active.
                    TrackingState::Idle(ref window) => {
                        if !is_idle() {
                            info!("User is active again, resuming timer for {:?}", window.class);
                            let now = chrono::Utc::now();
                            proc_data.resume(now);
                            state = TrackingState::Active(window.clone());
                        }
                    }
                    // if no window is in focus, do nothing.
                    TrackingState::NoFocus => {}
                }
            }

            _ = database_update.tick() => {
                proc_data.record_active_until(chrono::Utc::now());
                let rows = proc_data.drain_pending();
                if let Err(err) = backend.store_proc_data(&rows).await {
                    error!("Error sending data to procs table: {err:?}");
                }
            }

        }
    }
}

pub async fn run(update_interval: u32, backend: StorageBackend) -> Result<()> {
    let proc_data = ProcessTracker::new(backend.source_id(), backend.bucket_granularity_minutes());
    match detect_display_server() {
        DisplayServer::Wayland => {
            info!(
                "Wayland detected via environment. WAYLAND_DISPLAY={:?}, XDG_SESSION_TYPE={:?}, HYPRLAND_INSTANCE_SIGNATURE={:?}, DISPLAY={:?}",
                std::env::var("WAYLAND_DISPLAY").ok(),
                std::env::var("XDG_SESSION_TYPE").ok(),
                std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok(),
                std::env::var("DISPLAY").ok(),
            );
            #[cfg(feature = "wayland")]
            run_wayland(proc_data, update_interval, backend).await?;

            #[cfg(not(feature = "wayland"))]
            {
                error!("Running under Wayland but binary was built without `wayland` feature");
                return Err(anyhow!("Wayland feature not enabled"));
            }
        }
        DisplayServer::X11 => {
            info!(
                "X11 detected via environment. DISPLAY={:?}, XDG_SESSION_TYPE={:?}, WAYLAND_DISPLAY={:?}",
                std::env::var("DISPLAY").ok(),
                std::env::var("XDG_SESSION_TYPE").ok(),
                std::env::var("WAYLAND_DISPLAY").ok(),
            );
            #[cfg(feature = "x11")]
            run_x11(proc_data, update_interval, backend).await?;

            #[cfg(not(feature = "x11"))]
            {
                error!("Running under X11 but binary was built without `x11` feature, rebuild it with: `cargo build --features x11`");
                return Err(anyhow!("X11 feature not enabled"));
            }
        }
        DisplayServer::Unknown => {
            error!(
                "Could not determine graphical session type from environment. WAYLAND_DISPLAY={:?}, WAYLAND_SOCKET={:?}, XDG_SESSION_TYPE={:?}, HYPRLAND_INSTANCE_SIGNATURE={:?}, DISPLAY={:?}",
                std::env::var("WAYLAND_DISPLAY").ok(),
                std::env::var("WAYLAND_SOCKET").ok(),
                std::env::var("XDG_SESSION_TYPE").ok(),
                std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok(),
                std::env::var("DISPLAY").ok(),
            );
            return Err(anyhow!(
                "Failed to detect whether the session is Wayland or X11"
            ));
        }
    }

    anyhow::bail!("This should be unreachable");
}
