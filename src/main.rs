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

#[cfg(target_os = "windows")]
use crate::platform::windows::inputs::*;

#[cfg(feature = "remote")]
use crate::storage::remote::*;

use crate::storage::backend::*;
use crate::utils::args::Cli;
use crate::utils::lock::*;
use crate::utils::logger;

use anyhow::{Context, Result};
use clap::Parser;

use tokio::task::JoinSet;
use tracing::*;

#[cfg(target_os = "linux")]
mod input_bindings;

mod common;
mod platform;
mod storage;
mod utils;

#[tokio::main]
async fn main() {
    let args = Cli::parse();
    logger::init(args.debug);

    if let Err(err) = run(args).await {
        error!("Fatal Error: {err:?}");
    }
}

async fn run(mut args: Cli) -> Result<()> {
    // if we receive one of these two flags we call the function and it will enable or disable the
    // startup depending on the enable value.
    if args.enable_startup || args.disable_startup {
        let state = if args.enable_startup {
            "enable"
        } else {
            "disable"
        };

        configure_startup(&args).with_context(|| format!("Failed to {} startup", state))?;

        info!("Startup {}d successfully, the program will end now. Start it again without the start up flag to run normally.",state);
        return Ok(());
    }

    ensure_single_instance()
        .with_context(|| "Failed to ensure that we are the only instance of the program")?;

    info!(
        "Lock acquired. Running application with PID {}",
        std::process::id()
    );

    if args.debug && args.interval.is_none() {
        info!("Debug mode enabled but no interval was provided. Using default value of 5 seconds!");
        args.interval = 5.into();
    }

    let db_update_interval = args.interval.unwrap_or(300);

    #[cfg(not(feature = "remote"))]
    let storage_backend = StorageBackend::Local(
        LocalDb::new(args.gran, args.clear)
            .with_context(|| "Failed to initialize SQLite backend")?,
    );

    // We choose the API backend if user provide a path of the config with remote flag
    #[cfg(feature = "remote")]
    let storage_backend = if let Some(ref config_path) = args.remote {
        let api = RemoteDb::new(config_path).with_context(|| {
            format!("Failed to initialize API backend using config file: {config_path}")
        })?;
        StorageBackend::Api(api)
    } else {
        let db = LocalDb::new(args.gran, args.clear)
            .with_context(|| "Failed to initialize SQLite backend")?;
        StorageBackend::Local(db)
    };

    let mut tasks_set = JoinSet::new();
    #[cfg(target_os = "linux")]
    tasks_set.spawn(crate::platform::linux::inputs::run(
        args.dpi,
        db_update_interval + 5,
        storage_backend.clone(),
    ));

    #[cfg(target_os = "windows")]
    tasks_set.spawn(crate::platform::windows::inputs::run(
        args.dpi,
        db_update_interval + 5,
        storage_backend.clone(),
    ));

    tasks_set.spawn(process::run(db_update_interval, storage_backend));

    #[cfg(target_os = "windows")]
    if !args.no_systray {
        tasks_set.spawn(systray::init_tray());
    }

    // Need to wait the tasks finish, which they should'nt.
    while let Some(res) = tasks_set.join_next().await {
        // -> Option(Result(Result())))
        match res {
            Ok(Ok(())) => error!("Task exited cleanly but unexpectedly"),
            Ok(Err(err)) => return Err(err).with_context(|| "Task returned an error"),
            Err(join_err) => {
                return Err(anyhow::Error::new(join_err)).context("Task panicked or was cancelled")
            }
        }
    }

    Ok(())
}
