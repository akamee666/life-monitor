use clap::{value_parser, Parser};
use tokio::join;
use tracing::*;

// Define arguments that can be used by the program.
#[derive(Parser, Debug)]
#[command(name = "Life Monitor")]
#[command(about = "A program to monitor daily activity, see help for default behavior", long_about = None)]
pub struct Cli {
    /// Interval in seconds for logging data (default is 300 seconds).
    /// FIX:
    #[arg(
        short = 't',
        long,
        default_value_t = 300,
        help = "Set the interval (in seconds) for send the stored data to the database."
    )]
    interval: u32,

    /// Disable keys and mouse tracking.
    // FIX:
    #[arg(
        short = 'k',
        long,
        default_value_t = false,
        help = "If true, disables tracking of key presses and mouse movements. [default: false]",
        conflicts_with = "no_window"
    )]
    no_keys: bool,

    /// Disable tracking based on activity window
    // FIX:
    #[arg(
        short = 'w',
        long,
        default_value_t = false,
        help = "If true, disables tracking based on the currently active window. [default: false]",
        conflicts_with = "no_keys"
    )]
    no_window: bool,

    /// Disable systray icon
    // FIX:
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
        help = "If true, set the database update to occur more frequently and enable debug output to both log file and stdout, stdout output only works if RUST_LOG env is set to debug. [default: false]"
    )]
    debug: bool,

    // Enable remote database through api.
    // FIX: Link a explanation in github.
    #[arg(
        short = 'a',
        long,
        default_value = "false",
        help = "If true, enables updates to database through an api(BETA). [default: false]"
    )]
    api: bool,

    /// Specify DPI for mouse tracking.
    #[arg(
        short = 'p',
        long,
        default_value = "800",
        help = "This option is used to get better results when measuring how much you are moving your mouse. [default: 800]",
        conflicts_with = "no_keys",
        value_parser = value_parser!(u32).range(1..),
    )]
    dpi: u32,

    #[arg(
        long,
        default_value = "false",
        required = false,
        help = "Option used to delete all data collect in previous sessions of the program and start one from new. [default: false]"
    )]
    clean: bool,
}

#[tokio::main]
async fn main() {
    use life_monitor::localdb::clean_database;
    let args = Cli::parse();
    debug!("Arguments: {:?}", args);
    if args.clean {
        clean_database().unwrap();
    }
    run(args).await;
}

#[cfg(target_os = "linux")]
async fn run(args: Cli) {
    use life_monitor::{keylogger, linux::process, logger};
    logger::init(args.debug);

    // https://docs.rs/tokio/latest/tokio/macro.join.html
    // Running tasks in parallel, these will not finish.
    join!(process::init(), keylogger::init(args.dpi));
}

#[cfg(target_os = "windows")]
async fn run(mut args: Cli) {
    use life_monitor::{keylogger, logger, win::process, win::systray};
    logger::init(args.debug);
    if args.debug {
        args.interval = 5;
    }
    // https://docs.rs/tokio/latest/tokio/macro.join.html
    // Running tasks in parallel, these will not finish.
    join!(process::init(), keylogger::init(args.dpi), systray::init());
}
