use std::env;
use std::fs;
use std::fs::File;
use std::io;
use std::path::*;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;

use tracing::*;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::{fmt, prelude::*};

// Custom time formatter to display only hour, minute, and second
struct CustomTime;

impl FormatTime for CustomTime {
    fn format_time(&self, w: &mut fmt::format::Writer<'_>) -> std::fmt::Result {
        // this unwrap is also fine!
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        let secs = now.as_secs();
        let hours = (secs / 3600) % 24;
        let minutes = (secs / 60) % 60;
        let seconds = secs % 60;
        write!(w, "{:02}:{:02}:{:02}  ::", hours, minutes, seconds)
    }
}

// This function will define the level that logs will be displayed and also will create a file
// called spy.log in different paths depending on the platform.
pub fn init(enable_debug: bool) {
    if enable_debug {
        // We disable logs from other crates that also use tracing so we don't polute the log file/stdout
        // These unwraps are fine too
        let env_filter_std = EnvFilter::new("debug")
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("hyper_util=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap());

        // if debug is enable we should log everything. info, warn, error, debug.
        let env_filter_file = EnvFilter::new("debug")
            .add_directive("hyper=off".parse().unwrap())
            .add_directive("hyper_util=off".parse().unwrap())
            .add_directive("reqwest=off".parse().unwrap());

        registry(env_filter_file, env_filter_std, enable_debug);
    } else {
        // Display only error, info, and warns to stdout by default.
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
    // This doesn't need to propagate!
    if let Ok((file, path)) = create_file() {
        info!("Log file created at: {}", path.display());

        let file_layer = fmt::layer()
            .with_writer(Arc::new(file))
            .with_ansi(false)
            .with_target(false)
            .with_timer(CustomTime)
            .event_format(
                fmt::format()
                    .with_file(enable_debug)
                    .with_line_number(enable_debug)
                    .with_target(false),
            )
            .with_filter(env_filter_file);

        // yeah i am repeating this code bc otherwise it will fail with a error message with a hundred traits and 8 hundred types 1923123 lines long and aint
        // gonna try to fix it
        let stdout_layer = fmt::layer()
            .with_target(false)
            .without_time()
            .event_format(
                fmt::format()
                    .with_file(enable_debug)
                    .with_line_number(enable_debug)
                    .without_time()
                    .with_target(false),
            )
            .with_filter(env_filter_std);

        tracing_subscriber::registry()
            .with(file_layer)
            .with(stdout_layer)
            .init();
    } else {
        error!("Failed to create log file, logging to file is disabled.");

        let stdout_layer = fmt::layer()
            .with_target(false)
            .without_time()
            .event_format(
                fmt::format()
                    .with_file(enable_debug)
                    .with_line_number(enable_debug)
                    .without_time()
                    .with_target(false),
            )
            .with_filter(env_filter_std);

        tracing_subscriber::registry().with(stdout_layer).init();
    }
}

fn create_file() -> Result<(File, PathBuf)> {
    // Find a proper file to store the database in both os, create if already not exist
    let file = if cfg!(target_os = "windows") {
        let local_app_data = env::var("LOCALAPPDATA").map_err(|_| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "LOCALAPPDATA environment variable not set",
            )
        })?;
        let mut path = PathBuf::from(local_app_data);
        path.push("life-monitor");
        path.push("spy.log");

        if let Some(parent_dir) = path.parent() {
            fs::create_dir_all(parent_dir)?;
        }
        let f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.clone())?;

        (f, path)
    } else {
        let home_dir = env::var("HOME").map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, "HOME environment variable not set")
        })?;
        let mut path = PathBuf::from(home_dir);
        path.push(".local");
        path.push("share");
        path.push("life-monitor");
        path.push("spy.log");

        if let Some(parent_dir) = path.parent() {
            fs::create_dir_all(parent_dir)?;
        }
        let f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.clone())?;

        (f, path)
    };

    Ok(file)
}
