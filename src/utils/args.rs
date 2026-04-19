#[cfg(feature = "multi-sync")]
use clap::Subcommand;
use clap::{value_parser, Parser, ValueEnum};
use std::path::PathBuf;

use tracing::info;

use crate::common::DEFAULT_MOUSE_DPI;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReportKind {
    Sessions,
    Apps,
}

#[cfg(feature = "multi-sync")]
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum SyncCommand {
    Push,
    Pull,
    Status,
}

#[cfg(feature = "multi-sync")]
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    Sync {
        #[command(subcommand)]
        action: SyncCommand,
    },
}

#[derive(Parser, Debug)]
#[command(name = "Life Monitor")]
#[command(about = "Track keyboard, mouse, and focused-window activity into a SQLite database.")]
#[command(
    long_about = "Life Monitor records keyboard, mouse, scroll, and focused-window activity into a local SQLite database. It is designed for people who want to inspect, merge, export, or analyze their own activity data later.\n\nThe default workflow is simple: run the program, let it collect data in the background, and inspect or export the database whenever you need.\n\nUse --db-path to store the database somewhere else, such as another disk or a mounted network share. When you provide --db-path, Life Monitor remembers that location and reuses it on later runs until you choose another path."
)]
#[command(
    after_long_help = "Examples:\n  life-monitor\n  life-monitor --debug --interval 10\n  life-monitor --db-path /mnt/shared/life-monitor/data.db\n  life-monitor --export-db ./snapshot.sqlite\n  life-monitor --import-db ./snapshot.sqlite --dry-run\n  life-monitor --import-db ./snapshot.sqlite --import-notes \"desktop sync\""
)]
pub struct Cli {
    #[cfg(feature = "multi-sync")]
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(
        short = 'i',
        long,
        help_heading = "Collection",
        value_name = "SECS",
        help = "How often buffered activity is flushed to the database.",
        long_help = "Choose how often Life Monitor writes buffered activity buckets to SQLite.\n\nDefault: 300 seconds.\nDebug mode uses 5 seconds if you do not set an interval yourself.\n\nShorter intervals reduce the amount of data waiting in memory. Longer intervals reduce database writes."
    )]
    pub interval: Option<u32>,

    #[cfg(target_os = "windows")]
    #[arg(
        short = 's',
        long,
        help_heading = "Collection",
        default_value_t = false,
        help = "Windows only: disable the tray icon.",
        long_help = "Windows only.\n\nDisables the system tray icon and runs the program without that UI entry point."
    )]
    pub no_systray: bool,

    // WARN: Windows subsystem may affect this.
    //https://stackoverflow.com/questions/43744379/can-i-conditionally-compile-my-rust-program-for-a-windows-subsystem
    #[arg(
        short = 'd',
        long,
        help_heading = "Collection",
        default_value_t = false,
        help = "Enable verbose logging and a shorter default flush interval.",
        long_help = "Turns on more verbose logs and uses a 5-second flush interval if --interval is not provided.\n\nUseful for troubleshooting or when validating that collection is working."
    )]
    pub debug: bool,

    #[arg(
        long,
        help_heading = "Database",
        value_name = "PATH",
        help = "Store or read the SQLite database from a custom path.",
        long_help = "Use a specific SQLite database file instead of the default location.\n\nThis can point to:\n- a local file\n- another disk or partition\n- a mounted network share such as Samba or NFS\n\nWhen you provide this option, Life Monitor remembers the path and uses it again on later runs until you provide a different one.\n\nLife Monitor only uses paths that the operating system can already access. It does not mount remote shares or prompt for share credentials."
    )]
    pub db_path: Option<PathBuf>,

    #[arg(
        long,
        help_heading = "Import / Export",
        value_name = "FILE",
        help = "Export the current database into a consistent SQLite snapshot and exit.",
        long_help = "Creates a consistent snapshot of the current database at the given file path and then exits.\n\nThis uses SQLite backup primitives instead of copying the raw file directly."
    )]
    pub export_db: Option<PathBuf>,

    #[arg(
        long,
        help_heading = "Import / Export",
        value_name = "FILE",
        help = "Import a previously exported SQLite snapshot into the current database and exit.",
        long_help = "Imports a snapshot created by --export-db into the current database.\n\nThe import process validates both databases, creates a backup of the destination, merges the data, and records import metadata to prevent duplicate imports."
    )]
    pub import_db: Option<PathBuf>,

    #[arg(
        long,
        help_heading = "Import / Export",
        requires = "import_db",
        help = "Preview import changes without modifying the destination database.",
        long_help = "Shows what --import-db would add or update, without writing anything to the destination database."
    )]
    pub dry_run: bool,

    #[arg(
        long,
        help_heading = "Import / Export",
        value_name = "TEXT",
        requires = "import_db",
        help = "Attach optional notes to the recorded import metadata.",
        long_help = "Stores a free-form note alongside the import record. Useful for remembering where a snapshot came from or why it was imported."
    )]
    pub import_notes: Option<String>,

    #[arg(
        long,
        help_heading = "Analytics",
        value_enum,
        value_name = "KIND",
        conflicts_with_all = ["export_db", "import_db", "enable_startup", "disable_startup", "clear"],
        help = "Render a built-in analytics report and exit.",
        long_help = "Renders a built-in analytics report from the local SQLite database and then exits.\n\nAvailable reports:\n- sessions: collection sessions recorded by Life Monitor\n- apps: focused-app totals aggregated from focus buckets"
    )]
    pub report: Option<ReportKind>,

    #[arg(
        long,
        help_heading = "Analytics",
        requires = "report",
        value_name = "DAYS",
        default_value_t = 7,
        value_parser = value_parser!(u32).range(1..),
        help = "How many recent days the analytics report should cover."
    )]
    pub report_days: u32,

    #[arg(
        short = 'p',
        long,
        help_heading = "Collection",
        value_name = "DPI",
        help = "Mouse DPI/CPI used for estimating physical mouse distance in centimeters.",
        long_help = "Sets the mouse DPI/CPI used when converting raw mouse counts into estimated real-world distance.\n\nIf you provide this once, Life Monitor remembers it and reuses it on later runs until you provide a new value.\n\nIf you do not provide it and no remembered value exists, Life Monitor will ask for it on interactive runs.\n\nStart with 800 if you do not know your mouse DPI yet, then adjust later if needed.",
        value_parser = value_parser!(u32).range(1..),
    )]
    pub dpi: Option<u32>,

    #[arg(
        short = 'c',
        long,
        help_heading = "Database",
        help = "Delete the current database file and start from an empty one.",
        conflicts_with = "import_db",
        long_help = "Deletes the current database file before starting collection.\n\nUse this only if you really want to reset your data. The deletion is permanent."
    )]
    pub clear: bool,

    #[arg(
        long,
        help_heading = "Startup",
        help = "Enable automatic startup for the current user session.",
        long_help = "Configures Life Monitor to start automatically for the current user.\n\nOn Windows, this creates a shortcut in the Startup folder.\nOn Linux, this creates and enables a systemd --user service.\n\nThis is user-session startup, not a system-wide boot service.",
        conflicts_with = "disable_startup"
    )]
    pub enable_startup: bool,

    #[arg(
        long,
        help_heading = "Startup",
        help = "Disable automatic startup for the current user session.",
        long_help = "Removes Life Monitor from automatic startup for the current user.\n\nOn Windows, this removes the Startup shortcut.\nOn Linux, this stops and disables the systemd --user service and removes the unit file.",
        conflicts_with = "enable_startup"
    )]
    pub disable_startup: bool,

    #[cfg(feature = "multi-sync")]
    #[arg(
        long,
        help_heading = "Sync",
        default_value_t = false,
        help = "Enable background multi-device sync during normal collection runs."
    )]
    pub sync_enable: bool,

    #[cfg(feature = "multi-sync")]
    #[arg(
        long,
        help_heading = "Sync",
        value_name = "URL",
        help = "Remote sqld/libSQL endpoint used as the canonical sync store."
    )]
    pub sync_remote_url: Option<String>,

    #[cfg(feature = "multi-sync")]
    #[arg(
        long,
        help_heading = "Sync",
        value_name = "TOKEN",
        help = "Authentication token for the remote sqld/libSQL endpoint."
    )]
    pub sync_auth_token: Option<String>,

    #[cfg(feature = "multi-sync")]
    #[arg(
        long,
        help_heading = "Sync",
        value_name = "SECS",
        default_value_t = 300,
        value_parser = value_parser!(u64).range(1..),
        help = "How often to attempt push/pull sync while collecting."
    )]
    pub sync_interval: u64,
}

impl Cli {
    #[allow(dead_code)]
    pub fn print_args(&self) {
        info!("Arguments provided:");
        info!("Interval: {:?}", self.interval.unwrap_or(300));
        #[cfg(target_os = "windows")]
        info!("No systray: {:?}", self.no_systray);
        info!("Debug mode: {:?}", self.debug);
        info!("Database path: {:?}", self.db_path);
        info!("Export database: {:?}", self.export_db);
        info!("Import database: {:?}", self.import_db);
        info!("Dry-run import: {:?}", self.dry_run);
        info!("Report: {:?}", self.report);
        info!("Report days: {:?}", self.report_days);
        #[cfg(feature = "multi-sync")]
        info!("Sync command: {:?}", self.command);
        #[cfg(feature = "multi-sync")]
        info!("Sync enabled: {:?}", self.sync_enable);
        #[cfg(feature = "multi-sync")]
        info!("Sync remote URL: {:?}", self.sync_remote_url);
        info!("Mouse DPI: {:?}", self.dpi.unwrap_or(DEFAULT_MOUSE_DPI));
        info!("Clear database: {:?}", self.clear);
        println!();
    }
}
