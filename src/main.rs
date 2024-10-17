//#![windows_subsystem = "windows"]
// used to close the terminal and create a gui with no window.

use life_monitor::api::ApiConfig;
use life_monitor::args::Cli;
use life_monitor::data::*;
use life_monitor::localdb::{clean_database, open_con};
use life_monitor::logger;

use clap::Parser;

use reqwest::Client;

use tokio::task::JoinSet;

use tracing::*;

#[tokio::main]
async fn main() {
    let mut args = Cli::parse();
    logger::init(args.debug);
    args.print_args();

    if args.debug && args.interval.is_none() {
        info!("Debug is true but no interval value provided, using default five seconds!");
        args.interval = 5.into();
    }

    let use_api: Option<ApiConfig> = if let Some(ref config_path) = args.api {
        info!("Config file name: {:?}", config_path);

        let cfg = ApiConfig::from_file(config_path).unwrap_or_else(|err| {
            error!("Could not parse {config_path}. Error: {err}");
            panic!()
        });

        Some(cfg)
    } else {
        if args.clear {
            info!("Clean argument provided, cleaning database!");

            match clean_database() {
                Ok(_) => {}
                Err(e) => {
                    error!("Could not delete database, because of error: {e}. Most likely the database does not exist already, no need to crash.");
                }
            }
        };

        None
    };

    let storage_backend: StorageBackend = if use_api.is_some() {
        let api_config = use_api.unwrap();

        StorageBackend::Api(ApiStore::new(Client::new(), api_config))
    } else {
        let con = open_con().unwrap_or_else(|err| {
            error!("Could not open a connection with local database, quitting!\n Err: {err}",);
            panic!();
        });

        StorageBackend::Local(LocalDbStore::new(con))
    };

    run(args, storage_backend).await;
}

#[cfg(target_os = "linux")]
async fn run(args: Cli, backend: StorageBackend) {
    use life_monitor::keylogger;
    use life_monitor::linux::process;

    let backend2 = backend.clone();

    // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html
    let mut set = JoinSet::new();

    if !args.no_keys {
        set.spawn(keylogger::init(args.dpi, args.interval, backend));
    }

    if !args.no_window {
        set.spawn(process::init(args.interval, backend2));
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

#[cfg(target_os = "windows")]
async fn run(args: Cli, backend: StorageBackend) {
    use life_monitor::keylogger;
    use life_monitor::win::process;
    use life_monitor::win::systray;

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
