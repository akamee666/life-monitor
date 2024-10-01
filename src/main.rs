use clap::Parser;
use life_monitor::{keylogger, linux::process, logger};
use tokio::join;

// Define arguments that can be used by the program.
#[derive(Parser, Debug)]
#[command(name = "Life Monitor")]
#[command(about = "A program to monitor daily activity, see help for default behavior", long_about = None)]
struct Cli {
    /// Interval in seconds for logging data (default is 300 seconds)
    #[arg(
        short = 't',
        long,
        default_value_t = 300,
        help = "Set the interval (in seconds) for send the stored data to the database."
    )]
    interval: u64,

    /// Disable keys and mouse tracking
    #[arg(
        short = 'k',
        long,
        default_value_t = false,
        help = "If true, disables tracking of key presses and mouse movements. [default: false]",
        conflicts_with = "no_window"
    )]
    no_keys: bool,

    /// Disable tracking based on activity window
    #[arg(
        short = 'w',
        long,
        default_value_t = false,
        help = "If true, disables tracking based on the currently active window. [default: false]",
        conflicts_with = "no_keys"
    )]
    no_window: bool,

    /// Disable systray icon
    #[arg(
        short = 's',
        long,
        default_value_t = false,
        help = "If true, disables the system tray icon, this option is ignored in linux. [default: false]"
    )]
    no_systray: bool,

    // Enable debug to file.
    #[arg(
        short = 'd',
        long,
        default_value = "false",
        help = "If true, enables debug output to log file, RUST_LOG can be used for enable debug messages in stdout. [default: false]"
    )]
    debug: bool,

    // Enable remote database through api.
    #[arg(
        short = 'a',
        long,
        default_value = "false",
        help = "If true, enables updates to database through an api(BETA). [default: false]"
    )]
    // FIX: Link a explanation in github.
    api: bool,

    // Specify DPI for mouse tracking.
    #[arg(
        short = 'p',
        long,
        default_value = "0",
        required = true,
        help = "This option is required, if you don't know how much dpi you're using, use 0 as value to make a calibration test. [default: 800]",
        conflicts_with = "no_keys"
    )]
    dpi: u64,
}

#[tokio::main]
async fn main() {
    //debug!("Arguments: {:?}", args);

    let args = Cli::parse();
    run(args).await;
}

#[cfg(target_os = "linux")]
async fn run(args: Cli) {
    //FIX: UNIT TESTS TO LOGGER?
    logger::init(args.debug);

    // https://docs.rs/tokio/latest/tokio/macro.join.html
    // Running tasks in parallel, these will not finish.
    join!(process::init(), keylogger::init(args.dpi));
}

// FIX: REWRITE PROCESS LIKE LINUX.
#[cfg(target_os = "windows")]
async fn run(args: Cli) {
    logger::init(args.debug);

    // https://docs.rs/tokio/latest/tokio/macro.join.html
    // Running tasks in parallel, these will not finish.
    join!(process::init(), keylogger::init(), systray::init());
}
