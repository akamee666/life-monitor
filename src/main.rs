#[cfg(target_os = "windows")]
use crate::platform::windows::common::*;

#[cfg(target_os = "windows")]
use crate::platform::windows::process;

#[cfg(target_os = "windows")]
use crate::platform::windows::systray;

#[cfg(target_os = "linux")]
use crate::platform::linux::common::*;

#[cfg(target_os = "linux")]
use crate::platform::linux::process;

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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Cli::parse();
    logger::init(args.debug);

    // if we receive one of these two flags we call the function and it will enable or disable the
    // startup depending on the enable value.
    if args.enable_startup || args.disable_startup {
        let state = if args.enable_startup {
            "enabled"
        } else {
            "disabled"
        };

        configure_startup(&args).inspect_err(|_| {
            error!("Failed to {} startup", state);
        })?;

        info!("Startup {}d successfully, the program will end now. Start it again without the start up flag to run normally.",state);
        return Ok(());
    }

    if let Err(err) = ensure_single_instance() {
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            warn!("Already have one instance of life-monitor running!");
            return Ok(());
        }
        error!("Failed to ensure single instance when starting application");
        panic!("{err}");
    };

    debug!(
        "Lock acquired. Running application with PID {}",
        std::process::id()
    );

    if args.debug && args.interval.is_none() {
        info!("Debug is true but no interval value provided, using default value of 5 seconds!");
        args.interval = 5.into();
    }

    // We choose the API backend if user provide a path of the config with remote flag
    let storage_backend: StorageBackend = if let Some(ref config_path) = args.remote {
        let api = ApiStore::new(config_path).unwrap_or_else(|err| {
            error!("Failed to start API backend");
            panic!("Fatal error: {err}");
        });
        StorageBackend::Api(api)
    } else {
        let db = LocalDbStore::new(args.gran, args.clear).unwrap_or_else(|err| {
            error!("Failed to start SQLite backend");
            panic!("Fatal error: {err}");
        });
        StorageBackend::Local(db)
    };

    run(args, storage_backend).await;
    Ok(())
}

async fn run(args: Cli, backend: StorageBackend) {
    let backend2 = backend.clone();

    // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html
    let mut set = JoinSet::new();

    if !args.no_keys {
        // TODO: DOesn't working in wayland bc rdev doesn't support it
        set.spawn(keylogger::init(args.dpi, args.interval, backend));
    }

    if !args.no_window {
        set.spawn(process::init(args.interval, backend2));
    }

    #[cfg(target_os = "windows")]
    if !args.no_systray {
        set.spawn(systray::init());
    }

    // Need to wait the tasks finish, which they should not if everything is okay and i'm not dumb.
    // Without wait for them, run function will finish and all values will be droped, finishing the
    // entire program.
    while let Some(res) = set.join_next().await {
        match res {
            // That should not occur, i think?.
            Ok(_) => warn!("task has unexpectedly finished"),
            Err(err) => {
                error!("A task has panicked");
                panic!("fatal error in task: {err}")
            }
        }
    }
}
