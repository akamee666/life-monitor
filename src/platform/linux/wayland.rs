use crate::common::*;

use tokio::sync::mpsc::*;
use tokio::time::*;

use anyhow::*;
use tracing::*;

use std::io;
use std::sync::Arc;

use wayland_client::backend::ObjectData;
use wayland_client::{protocol::wl_registry, Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::*, zwlr_foreign_toplevel_manager_v1::*, *,
};

struct WaylandData {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    windows: Vec<Window>,
    channel_sender: Sender<FocusEvent>,
}

#[derive(Debug, Clone)]
pub struct Window {
    wl_handle: ZwlrForeignToplevelHandleV1,
    pub w_name: String,
    pub w_class: String,
}

#[derive(Debug, Clone)]
pub enum FocusEvent {
    FocusGained(Window),
    FocusLost(Window),
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
                // Adding existent windows
                state.windows.push(Window {
                    wl_handle: toplevel,
                    w_name: "".to_string(),
                    w_class: "".to_string(),
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
                    w.w_name = title;
                }
            }
            Event::AppId { app_id } => {
                if let Some(window) = state.windows.iter_mut().find(|t| t.wl_handle == *handle) {
                    window.w_class = app_id;
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
                        state
                            .channel_sender
                            .try_send(FocusEvent::FocusGained(focused_window.clone()))
                            .unwrap();
                    }
                } else if let Some(unfocused_window) =
                    state.windows.iter().find(|t| t.wl_handle == *handle)
                {
                    state
                        .channel_sender
                        .try_send(FocusEvent::FocusLost(unfocused_window.clone()))
                        .unwrap();
                }
            }

            Event::Closed => {
                if let Some(closed_window) = state.windows.iter().find(|t| t.wl_handle == *handle) {
                    debug!("Window [{}] was closed", closed_window.w_class);
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

pub fn listen_for_wlevents(sender: Sender<FocusEvent>) -> Result<()> {
    let conn = Connection::connect_to_env()
        .with_context(|| "Failed to open a connection with wayland server")?;
    let wl_display = conn.display();
    let mut event_q = conn.new_event_queue();
    let qh = event_q.handle();
    let _ = wl_display.get_registry(&qh, ());
    let mut wl_data = WaylandData {
        manager: None,
        windows: Vec::with_capacity(300),
        channel_sender: sender,
    };

    // Flush pending requests from wayland server.
    // Our interface for Wl_registry will receive the events and bind to the interface we need.
    event_q
        .roundtrip(&mut wl_data)
        .with_context(|| "Error while requesting global interfaces from wayland server")?;

    // If we didn't fail before and still don't have a manager, the user compositor don't support
    // the interface we want.
    if wl_data.manager.is_none() {
        error!("The interface 'zwlr_foreign_toplevel_manager_v1' is not currently advertised by your compositor");
        return Err(std::io::Error::from(io::ErrorKind::Unsupported).into());
    }

    event_q.roundtrip(&mut wl_data).with_context(|| {
        "Interface 'zwlr_foreign_toplevel_manager_v1' is available but we failed when requesting it"
    })?;

    loop {
        // We block the thread to receive events from the toplevel handle :D
        event_q
            .blocking_dispatch(&mut wl_data)
            .with_context(|| "Failed when handling events from wayland interface: {err}")?;
    }
}

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
