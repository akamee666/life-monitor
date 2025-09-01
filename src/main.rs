//#![windows_subsystem = "windows"]
// used to close the terminal and create a gui with no window in Windows.

#[cfg(target_os = "windows")]
use crate::platform::windows::common::*;

#[cfg(target_os = "linux")]
use crate::platform::linux::common::*;

use crate::storage::backend::*;
use crate::utils::args::Cli;
use crate::utils::lock::*;
use crate::utils::logger;

use clap::Parser;

use tokio::task::JoinSet;
use tracing::*;

mod common;
mod keylogger;
mod platform;
mod storage;
mod utils;

#[tokio::main]
async fn main() {
    let mut args = Cli::parse();
    logger::init(args.debug);

    let _lock = ensure_single_instance().unwrap_or_else(|e| {
        error!("Failed to acquire lock: {}", e);
        panic!();
    });

    debug!(
        "Lock acquired. Running application with PID {}",
        std::process::id()
    );

    // if we receive one of these two flags we call the function and it will enable or disable the
    // startup depending on the enable value.
    let r = check_startup_status().unwrap_or_else(|err| {
        error!("Failed to check startup status.");
        panic!("{err}");
    });
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
    use crate::keylogger;
    use crate::platform::linux::process;

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
    use crate::keylogger;
    use crate::platform::windows::process;
    use crate::platform::windows::systray;

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
