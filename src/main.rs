//#![windows_subsystem = "windows"]
// used to close the terminal and create a gui with no window in Windows.

#[cfg(target_os = "windows")]
use life_monitor::platform::win::util::configure_startup;

#[cfg(target_os = "linux")]
use life_monitor::platform::linux::util::configure_startup;

use life_monitor::args::Cli;
use life_monitor::backend::*;
use life_monitor::is_startup_enable;
use life_monitor::lock::ensure_single_instance;
use life_monitor::logger;

use clap::Parser;

use tokio::task::JoinSet;
use tracing::*;

use wayland_client::{protocol::wl_registry, Connection, Dispatch, QueueHandle};

use wayland_protocols_wlr::foreign_toplevel::v1::client::{ *, zwlr_foreign_toplevel_handle_v1::*, zwlr_foreign_toplevel_manager_v1::*};

#[derive(Debug)]
struct TopLevel {
    handle: ZwlrForeignToplevelHandleV1,
    title: Option<String>,
    app_id: Option<String>,
    is_active: bool,
}

struct AppData {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    toplevels: Vec<TopLevel>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for AppData {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<AppData>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            println!("[{}] {}, (v{})", name, interface, version);
            match &interface[..] {
                "zwlr_foreign_toplevel_manager_v1" => {
                    let toplevel_manager = registry.bind::<ZwlrForeignToplevelManagerV1,_,_>(name,3,qh,());
                    state.manager = Some(toplevel_manager);
                },
                _ => {},
            }

        }
    }
}


use std::sync::Arc;
use wayland_client::backend::ObjectData;

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _mgr: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<AppData>,
    ) {
        use zwlr_foreign_toplevel_manager_v1::Event;

        match event {
            Event::Toplevel { toplevel } => {
                println!("New toplevel handle: {:?}", toplevel);
                state.toplevels.push(TopLevel {
                    handle: toplevel,
                    title: None,
                    app_id: None,
                    is_active: false,
                });
            }
            Event::Finished => {
                println!("Manager: initial toplevels sent");
            }
            _ => {}
        }
    }


    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<Self>,
    ) -> Arc<dyn ObjectData> {
        match opcode {
            0 => qhandle.make_data::<ZwlrForeignToplevelHandleV1, _>(()),  
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for AppData {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<AppData>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        match event {
            Event::Title { title } => {
                println!("Window title: {}", title);
                if let Some(w) = state.toplevels.iter_mut().find(|t| t.handle == *handle) {
                    w.title = Some(title);
                }
            }
            Event::AppId { app_id } => {
                println!("App ID: {}", app_id);
            }
            Event::State { state: states } => {
                println!("State bits: {:?}", states);
            }
            Event::Closed => {
                println!("Window closed");
            }
            _ => {}
        }
    }
}


#[tokio::main]
async fn main() {
    let mut args = Cli::parse();
    logger::init(args.debug);

    let conn = Connection::connect_to_env().unwrap();
    let wl_display = conn.display();
    let mut event_q = conn.new_event_queue();
    let qh = event_q.handle();

    let _registry =  wl_display.get_registry(&qh,());

    info!("Advertised globals:");

    let mut state = AppData {
        manager: None,
        toplevels: Vec::new(),
    };

    // 1: discover globals
    event_q.roundtrip(&mut state).unwrap();

    // 2: receive initial toplevels 
    event_q.roundtrip(&mut state).unwrap();

    loop {
        event_q.blocking_dispatch(&mut state).unwrap();
    }

    let _lock = ensure_single_instance().unwrap_or_else(|e| {
        error!("Failed to acquire lock: {}", e);
        std::process::exit(1);
    });

    debug!(
        "Lock acquired. Running application with PID {}",
        std::process::id()
    );

    // if we receive one of these two flags we call the function and it will enable or disable the
    // startup depending on the enable value.
    let r = is_startup_enable().unwrap();
    if args.enable_startup || args.disable_startup {
        match configure_startup(args.enable_startup, r) {
            Ok(_) => {
                info!(
                    "Startup configuration {} successfully, the program will end now. Start it again without the start up flag to run normally.",
                    if args.enable_startup {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
                return;
            }
            Err(e) => {
                error!("Failed to configure startup: {}", e);
                return;
            }
        }
    }

    if args.debug && args.interval.is_none() {
        info!("Debug is true but no interval value provided, using default five seconds!");
        args.interval = 5.into();
    }

    // If args.api, which is a Option<String> that should be the path to API config file, is Some
    // then we use it as backend.
    let storage_backend: StorageBackend = if let Some(ref config_path) = args.remote {
        match ApiStore::new(config_path) {
            Ok(api) => StorageBackend::Api(api),
            Err(e) => {
                error!("Failed to initialize API backend due to {e}.");
                return;
            }
        }
    } else {
        let db = match LocalDbStore::new(args.gran, args.clear) {
            Ok(db) => db,
            Err(e) => {
                error!("Failed to initialize SQLite backend due to {e}.");
                return;
            }
        };
        StorageBackend::Local(db)
    };

    run(args, storage_backend).await;
}

#[cfg(target_os = "linux")]
async fn run(args: Cli, backend: StorageBackend) {
    use life_monitor::keylogger;
    use life_monitor::platform::linux::process;

    let backend2 = backend.clone();

    // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html
    let mut set = JoinSet::new();

    if !args.no_keys {
        set.spawn(keylogger::init(args.dpi, args.interval, backend));
    }

    if !args.no_window {
        set.spawn(process::init(args.interval, backend2));
    }

    // Need to wait the tasks finish, which they should not if everything is okay and i'm not dumb.
    // Without wait for them, run function will finish and all values will be droped, finishing the
    // entire program.
    while let Some(res) = set.join_next().await {
        match res {
            // That should not occur.
            Ok(_) => error!("A task has unexpectedly finished"),
            // panicked!
            Err(e) => {
                error!("A task has panicked: {}", e);
                panic!()
            }
        }
    }
}

#[cfg(target_os = "windows")]
async fn run(args: Cli, backend: StorageBackend) {
    use life_monitor::keylogger;
    use life_monitor::platform::win::process;
    use life_monitor::platform::win::systray;

    let backend2 = backend.clone();

    // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html
    let mut set = JoinSet::new();

    if !args.no_keys {
        set.spawn(keylogger::init(args.dpi, args.interval, backend));
    }

    if !args.no_window {
        set.spawn(process::init(args.interval, backend2));
    }

    if !args.no_systray {
        set.spawn(systray::init());
    }

    // Need to wait the tasks finish, which they should not if there is no error.
    // Without wait for them, run function will finish and all values will be droped, finishing the
    // entire program.
    while let Some(res) = set.join_next().await {
        match res {
            // That should not occur.
            Ok(_) => error!("A task has unexpectedly finished"),
            // panicked!
            Err(e) => {
                error!("A task has panicked: {}", e);
                panic!()
            }
        }
    }
}
