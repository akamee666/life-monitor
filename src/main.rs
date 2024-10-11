#![windows_subsystem = "windows"]
// used to close the terminal.

use clap::{value_parser, Parser};
use tokio::task::JoinSet;
use tracing::*;

#[derive(Parser, Debug)]
#[command(name = "Life Monitor")]
#[command(about = "A program to monitor daily activity, see help for default behavior")]
#[command(
    long_about = "Life Monitor is a comprehensive tool designed to track and analyze your daily computer usage. It monitors various aspects of your activity, including keyboard and mouse input, active windows, and overall process usage(No, i am not a spyware i swear). This data can be used to gain insights into your productivity, work patterns, and computer usage habits. The program does not provide any overview for the collected data, it just collect and save them to the database file in your respective path, use them as you wish."
)]
pub struct Cli {
    #[arg(
        short = 't',
        long,
        help = "Set the interval (in seconds) for sending stored data to the database.",
        long_help = "This option allows you to specify how often the program should send the collected data to the database. The default is every 300 seconds (5 minutes). Shorter intervals will update the database more frequently but may increase system load, while longer intervals will reduce database updates but may delay data availability. Choose an interval that balances your need for up-to-date information with system performance considerations. This option overwrite the interval used by the debug flag, if you want debug information and does not want to change interval, use both."
    )]
    interval: Option<u32>,

    #[arg(
        short = 'k',
        long,
        default_value_t = false,
        help = "If true, disables tracking of key presses and mouse movements. [default: false]",
        long_help = "This option allows you to disable the tracking of keyboard and mouse input. When enabled, the program will not record any information about key presses or mouse movements. This can be useful if you want to monitor only window activity without detailed input tracking, or if you have privacy concerns about logging keystrokes. Note that this option conflicts with the --no-window option, as at least one type of tracking must be enabled.",
        conflicts_with = "no_window"
    )]
    no_keys: bool,

    #[arg(
        short = 'w',
        long,
        default_value_t = false,
        help = "If true, disables tracking based on the currently active window. [default: false]",
        long_help = "This option disables the tracking of active windows. When enabled, the program will not record information about which applications or windows are in use. This can be useful if you only want to track overall input activity without associating it with specific applications. Note that this option conflicts with the --no-keys option, as at least one type of tracking must be enabled.",
        conflicts_with = "no_keys"
    )]
    no_window: bool,

    #[cfg(target_os = "windows")]
    #[arg(
        short = 's',
        long,
        default_value_t = false,
        help = "If true, disables the system tray icon. [default: false]",
        long_help = "This option is only available on Windows systems. When enabled, it prevents the program from creating an icon in the system tray. The system tray icon provides quick access to the program's status and controls, so disabling it may make the program less convenient to interact with. However, it can be useful if you prefer to run the program without any visible interface. On non-Windows systems, this option is not available, and no system tray icon will be created."
    )]
    no_systray: bool,

    // WARN: Windows subsytem may fuck this.
    #[arg(
        short = 'd',
        long,
        default_value_t = false,
        help = "If true, enables debug mode with more frequent updates and additional logging.",
        long_help = "Enabling debug mode does two things: First, it increases the frequency of database updates, allowing for more real-time data analysis. Second, it enables debug output to both a log file and stdout. Note that stdout output only works if the RUST_LOG environment variable is set to 'debug'. This mode is useful for troubleshooting issues or for developers working on extending the program's functionality. Interval option WILL overwrite the interval defined by this option."
    )]
    debug: bool,

    // FIX: Handle this one.
    //#[arg(
    //    short = 'a',
    //    long,
    //    default_value_t = false,
    //    help = "If true, enables updates to database through an API (BETA). [default: false]",
    //    long_help = "This BETA feature enables the program to update a remote database through an API instead of or in addition to the local database. This can be useful for centralized data collection or for accessing your data from multiple devices. However, as this is a beta feature, it may not be as stable or secure as the local database option. It's usable but only for a specific case see the explanation in github page if you still want to use it anyway"
    //)]
    //api: bool,
    #[arg(
        short = 'p',
        long,
        help = "Specify the DPI setting of your mouse for accurate movement tracking. [default: 800]",
        long_help = "This option allows you to specify the DPI (dots per inch) setting of your mouse. Providing the correct DPI value helps the program accurately measure how much you're moving your mouse. A higher DPI means the mouse is more sensitive and moves the cursor further for the same physical movement. The default value is 800 DPI, which is common for many mice. Check your mouse settings or manufacturer specifications to find the correct DPI value. This option conflicts with --no-keys, as mouse tracking is part of the key and mouse input tracking feature.",
        conflicts_with = "no_keys",
        value_parser = value_parser!(u32).range(1..),
    )]
    dpi: Option<u32>,

    #[arg(
        short = 'c',
        long,
        default_value_t = false,
        help = "If true, deletes all previously collected data and starts fresh. [default: false]",
        long_help = "This option, when enabled, will delete all data collected in previous sessions of the program and start with a clean slate. This can be useful if you want to reset your tracking, perhaps after a significant change in your work habits or if you suspect there are issues with the existing data. Be very careful when using this option, as it will permanently delete all existing data. It's recommended to backup your data before using this option."
    )]
    clean: bool,
}

#[tokio::main]
async fn main() {
    use life_monitor::localdb::clean_database;
    use life_monitor::logger;

    let mut args = Cli::parse();

    // The target os doesn't matter in these two fn.
    logger::init(args.debug);
    debug!("Arguments: {:?}", args);
    if args.clean {
        clean_database().unwrap();
    }

    // Only change interval to five for debug reasons if inverval option is not provided.
    if args.debug || args.interval.is_none() {
        args.interval = 5.into();
    }

    run(args).await;
}

#[cfg(target_os = "linux")]
async fn run(args: Cli) {
    use life_monitor::{keylogger, linux::process};

    // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html
    let mut set = JoinSet::new();

    if !args.no_keys {
        set.spawn(keylogger::init(args.dpi, args.interval));
    }

    if !args.no_window {
        set.spawn(process::init(args.interval));
    }

    // Without this, the tasks do not run at all.
    // I guess it's because i do not wait for them to finish, so even they should run forever i
    // still need to wait for a response for them forever as well.
    while let Some(res) = set.join_next().await {
        match res {
            // That should not occur.
            Ok(_) => error!("A task has unexpectedly finished"),
            // panicked!
            Err(e) => error!("A task has panicked: {}", e),
        }
    }
}

#[cfg(target_os = "windows")]
async fn run(args: Cli) {
    use life_monitor::{keylogger, win::process, win::systray};

    // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html
    let mut set = JoinSet::new();

    if !args.no_keys {
        set.spawn(keylogger::init(args.dpi, args.interval));
    }

    if !args.no_window {
        set.spawn(process::init(args.interval));
    }

    if !args.no_systray {
        set.spawn(systray::init());
    }

    // Without this, the tasks do not run at all.
    // I guess it's because i do not wait for them to finish, so even they should run forever i
    // still need to wait for a response for them forever as well.
    while let Some(res) = set.join_next().await {
        match res {
            // That should not occur.
            Ok(_) => error!("A task has unexpectedly finished"),
            // panicked!
            Err(e) => error!("A task has panicked: {}", e),
        }
    }
}
