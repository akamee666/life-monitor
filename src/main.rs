use keylogger::KeyLogger;

mod db;
mod keylogger;
mod logger;
mod shutdown;
mod win;

#[tokio::main]
async fn main() {
    logger::init_logger();

    // This event will *only* be recorded by the metrics layer.
    tracing::info!(target: "metrics::cool_stuff_count", value = 42);

    // This event will only be seen by the debug log file layer:
    tracing::debug!("this is a message, and part of a system of messages");

    // This event will be seen by both the stdout log layer *and*
    // the debug log file layer, but not by the metrics layer.
    tracing::warn!("the message is a warning about danger!");

    // TODO: FIX DB ISSUES
    // TODO: I SUCK AT CODING NEED TO REFACTOR A LOT>
    // TODO: LOGGER in example seems good enough?
    tokio::spawn(crate::win::systray::init());
    tokio::spawn(crate::win::process::ProcessTracker::track_processes());
    KeyLogger::start_logging().await;
}
