use std::env;
use std::fs;
use std::fs::File;
use std::io;
use std::path::*;
use std::sync::Arc;

use tracing::info;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::{filter, fmt, prelude::*};

// This function will define the level that logs will be displayed and also will create a file
// called spy.log in different paths depending on the platform.
pub fn init(enable_debug: bool) {
    if enable_debug {
        let env_filter_std = EnvFilter::new("debug")
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("hyper_util=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap());

        // Display info, warn, error, and debug prints to the file by default.
        let env_filter_file = EnvFilter::new("debug")
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("hyper_util=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap());

        registry(env_filter_file, env_filter_std, enable_debug);
    } else {
        // Display only error, info, and warns to stdout by default, use RUST_LOG to change filter.
        // WARN: RUST_LOG="debug" is the only that doesn't work
        let env_filter_std = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"))
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("hyper_util=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap());

        // Display only error, info, and warns to file.
        let env_filter_file = EnvFilter::new("info")
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("hyper_util=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap());

        registry(env_filter_file, env_filter_std, enable_debug);
    }
}

fn registry(env_filter_file: EnvFilter, env_filter_std: EnvFilter, enable_debug: bool) {
    // A layer that logs events to a file.
    let file = match create_file() {
        Ok(file) => file,
        Err(error) => panic!("Error: {error}"),
    };

    // Configure the stdout log format based on the `enable_debug` flag.
    let stdout_log = fmt::layer().with_target(false).without_time().event_format(
        fmt::format()
            .with_file(enable_debug) // Include file name only if debug is enabled
            .with_line_number(enable_debug) // Include line number only if debug is enabled
            .without_time()
            .with_target(false),
    );

    // Configure the debug log format based on the `enable_debug` flag.
    let debug_log = fmt::layer()
        .with_writer(Arc::new(file))
        .with_ansi(false)
        .with_target(false)
        .without_time()
        .event_format(
            fmt::format()
                .with_file(enable_debug) // Include file name only if debug is enabled
                .with_line_number(enable_debug) // Include line number only if debug is enabled
                .without_time()
                .with_target(false),
        );

    // A layer that collects metrics using specific events.
    let metrics_layer = /* ... */ filter::LevelFilter::INFO;
    tracing_subscriber::registry()
        .with(
            stdout_log
                // Add an `INFO` filter to the stdout logging layer
                .with_filter(env_filter_std)
                // Combine the filtered `stdout_log` layer with the
                // `debug_log` layer, producing a new `Layered` layer.
                .and_then(debug_log)
                .with_filter(env_filter_file)
                // Add a filter to *both* layers that rejects spans and
                // events whose targets start with `metrics`.
                .with_filter(filter::filter_fn(|metadata| {
                    !metadata.target().starts_with("metrics")
                })),
        )
        .with(
            // Add a filter to the metrics label that *only* enables
            // events whose targets start with `metrics`.
            metrics_layer.with_filter(filter::filter_fn(|metadata| {
                metadata.target().starts_with("metrics")
            })),
        )
        .init();

    info!("Log file created!");
}

fn create_file() -> Result<File, std::io::Error> {
    // Find a proper file to store the database in both os, create if already not exist
    let file = if cfg!(target_os = "windows") {
        let local_app_data = env::var("LOCALAPPDATA").map_err(|_| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "LOCALAPPDATA environment variable not set",
            )
        })?;
        let mut path = PathBuf::from(local_app_data);
        path.push("akame_monitor");
        path.push("spy.log");

        if let Some(parent_dir) = path.parent() {
            fs::create_dir_all(parent_dir)?;
        }

        fs::File::create(path).unwrap()
    } else {
        let home_dir = env::var("HOME").map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, "HOME environment variable not set")
        })?;
        let mut path = PathBuf::from(home_dir);
        path.push(".local");
        path.push("share");
        path.push("akame_monitor");
        path.push("spy.log");

        if let Some(parent_dir) = path.parent() {
            fs::create_dir_all(parent_dir)?;
        }

        fs::File::create(path).unwrap()
    };

    Ok(file)
}
