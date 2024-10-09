use std::{fs::File, sync::Arc};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::{filter, prelude::*};
pub fn init(enable_debug: bool) {
    if enable_debug {
        // Display only error and warns to stdout by default, use RUST_LOG to change filter.
        let env_filter_std =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

        // Display info, warn, error and debug prints to the file by default.
        let env_filter_file = EnvFilter::new("debug");

        registry(env_filter_file, env_filter_std);
    } else {
        // Display only error and warns to stdout by default, use RUST_LOG to change filter.
        let env_filter_std =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

        // Display only error and warns to file.
        let env_filter_file = EnvFilter::new("warn");

        registry(env_filter_file, env_filter_std);
    }
}

fn registry(env_filter_file: EnvFilter, env_filter_std: EnvFilter) {
    // A layer that logs events to a file.
    let file = File::create("logs");

    let file = match file {
        Ok(file) => file,
        Err(error) => panic!("Error: {:?}", error),
    };

    let stdout_log = tracing_subscriber::fmt::layer()
        .with_target(false)
        .without_time();

    let debug_log = tracing_subscriber::fmt::layer()
        .with_writer(Arc::new(file))
        .with_ansi(false)
        .with_target(false);

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
}
