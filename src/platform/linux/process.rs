use crate::common::Event;
use crate::common::*;
use crate::platform::common::*;
use crate::storage::backend::{DataStore, StorageBackend};

use std::sync::Arc;

use tokio::sync::mpsc::channel;
use tokio::sync::Mutex;
use tokio::time::Duration;

use tracing::*;

use wayland_client::{protocol::wl_registry, Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::*, zwlr_foreign_toplevel_manager_v1::*, *,
};

struct WaylandData {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    windows: Vec<Window>,
}

struct Window {
    wl_handle: ZwlrForeignToplevelHandleV1,
    w_name: Option<String>,
    w_class: Option<String>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandData {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<WaylandData>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if &interface[..] == "zwlr_foreign_toplevel_manager_v1" {
                let toplevel_manager =
                    registry.bind::<ZwlrForeignToplevelManagerV1, _, _>(name, version, qh, ());
                state.manager = Some(toplevel_manager.clone());
                debug!("Interface found and binded successfully!");
            }
        }
    }
}

use wayland_client::backend::ObjectData;
impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for WaylandData {
    fn event(
        state: &mut Self,
        _mgr: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<WaylandData>,
    ) {
        use zwlr_foreign_toplevel_manager_v1::Event;

        match event {
            Event::Toplevel { toplevel } => {
                state.windows.push(Window {
                    wl_handle: toplevel,
                    w_name: None,
                    w_class: None,
                });
            }
            Event::Finished => {
                println!("Manager: we are finished");
            }
            _ => {}
        }
    }

    fn event_created_child(opcode: u16, qhandle: &QueueHandle<Self>) -> Arc<dyn ObjectData> {
        match opcode {
            0 => qhandle.make_data::<ZwlrForeignToplevelHandleV1, _>(()),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for WaylandData {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<WaylandData>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;

        match event {
            Event::Title { title } => {
                // Each window/toplevel has a different handle, this is how we can know
                // from each window we are receiving events.
                if let Some(w) = state.windows.iter_mut().find(|t| t.wl_handle == *handle) {
                    w.w_name = Some(title);
                }
            }
            Event::AppId { app_id } => {
                // debug purposes only!
                let windows_n = state.windows.len();
                if let Some(window) = state.windows.iter_mut().find(|t| t.wl_handle == *handle) {
                    debug!("Adding window [{app_id}], number of windows is: {windows_n}");
                    window.w_class = Some(app_id);
                }
            }
            Event::State { state: w_state } => {
                let states: Vec<u32> = w_state
                    .chunks(4)
                    .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
                    .collect();
                if states.contains(&2) {
                    if let Some(focused_window) =
                        state.windows.iter().find(|t| t.wl_handle == *handle)
                    {
                        debug!(
                            "The window [{}] is currently in focus",
                            focused_window.w_class.as_deref().unwrap_or("N/A")
                        );
                    }
                } else {
                    // You can also handle when a window loses focus.
                    if let Some(_unfocused_window) =
                        state.windows.iter().find(|t| t.wl_handle == *handle)
                    {}
                }
            }

            Event::Closed => {
                if let Some(closed_window) = state.windows.iter().find(|t| t.wl_handle == *handle) {
                    debug!(
                        "Window [{}] was closed",
                        closed_window.w_class.as_deref().unwrap_or("N/A")
                    );
                }
            }

            Event::OutputEnter { output } => {
                debug!("Output enter: {output:?}");
            }

            Event::OutputLeave { output } => {
                debug!("Output leave: {output:?}");
            }

            Event::Parent { parent } => {
                debug!("Parent: {parent:?}");
            }

            _ => {}
        }
    }
}

pub async fn init(interval: Option<u32>, backend: StorageBackend) {
    // Adding some difference between the two tasks so they don't try to call database at the
    // same time and waste time waiting for lock.
    let db_int = if let Some(interval) = interval {
        info!("Interval argument provided, changing values.");
        interval + 5
    } else {
        300 + 5
    };

    let logger = Arc::new(Mutex::new(ProcessTracker::new(&backend).await));
    let (tx, mut rx) = channel(20);

    let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();

    // Wayland is event driven already, we don't need an event to check the current window
    // every second, instead, we just listen for events from the w server.
    if is_wayland {
        tokio::spawn(async move {
            let conn = Connection::connect_to_env().unwrap_or_else(|err| {
                error!("Failed to open a connection with wayland server.");
                panic!("{err}");
            });
            let wl_display = conn.display();
            let mut event_q = conn.new_event_queue();
            let qh = event_q.handle();
            let _ = wl_display.get_registry(&qh, ());
            // We start empty
            let mut wl_data = WaylandData {
                manager: None,
                windows: Vec::with_capacity(300),
            };

            // Flush pending requests from wayland server.
            // Our interface for Wl_registry will receive the events and bind to the interface we need.
            event_q.roundtrip(&mut wl_data).unwrap_or_else(|err| {
                error!("Error while requesting global interfaces from wayland server");
                panic!("{err}");
            });

            // If we didn't fail before and still don't have a manager, the user compositor don't support
            // the interface we want.
            if wl_data.manager.is_none() {
                error!("The interface 'zwlr_foreign_toplevel_manager_v1' is not currently advertised by your compositor");
                panic!();
            }

            event_q.roundtrip(&mut wl_data).unwrap_or_else(|err| {
                error!("Interface 'zwlr_foreign_toplevel_manager_v1' is available but we failed when requesting it");
                panic!("{err}");
            });

            loop {
                // We block the thread to receive events from the toplevel handle :D
                event_q
                    .blocking_dispatch(&mut wl_data)
                    .unwrap_or_else(|err| {
                        error!("Failed when handling events from wayland interface: {err}");
                        0
                    });
            }
        });
    };

    if !is_wayland {
        // This event will send events each one second so we can track the active window in x11.
        spawn_ticker(tx.clone(), Duration::from_secs(1), Event::Tick);
        // Each twenty seconds we gonna check if user is idle
        // Since wayland is event driven, we don't need a event to check if user is idle or not.
        // TODO: Or maybe we do?
        spawn_ticker(tx.clone(), Duration::from_secs(20), Event::IdleCheck);
    }
    // Each [interval here] seconds we gonna send updates to the database
    spawn_ticker(
        tx.clone(),
        Duration::from_secs(db_int.into()),
        Event::DbUpdate,
    );

    let mut idle = false;
    while let Some(event) = rx.recv().await {
        match event {
            Event::Tick => {
                if !idle {
                    let mut tracker = logger.lock().await;
                    handle_active_window(&mut tracker).await;
                }
            }
            Event::IdleCheck => {
                let tracker = logger.lock().await;
                idle = is_idle(&tracker.idle_period);
            }
            Event::DbUpdate => {
                let tracker = logger.lock().await;
                if let Err(e) = backend.store_proc_data(&tracker.procs).await {
                    error!("Error sending data to procs table. Error: {e}");
                }
            }
        }
    }
}
