use clap::{
    builder::styling::{AnsiColor, Effects, Styles},
    value_parser, Args, CommandFactory, FromArgMatches, Parser, Subcommand,
};
use std::path::PathBuf;

use tracing::info;

use crate::common::DEFAULT_MOUSE_DPI;

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxStartupMode {
    Xdg,
    Systemd,
}

#[cfg(feature = "multi-sync")]
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum SyncCommand {
    Push,
    Pull,
    Status,
}

#[derive(Debug, Clone, Args)]
#[command(about = "Run the background collector and all collector-related maintenance commands.")]
pub struct CollectorCli {
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
        long_help = "Windows only.\n\nDisables the system tray icon and runs the collector without that UI entry point."
    )]
    pub no_systray: bool,

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
        help = "Store or read the collector SQLite database from a custom path.",
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
        long_help = "Configures Life Monitor to start automatically in collector mode for the current user.\n\nOn Windows, this creates a shortcut in the Startup folder.\nOn Linux, Life Monitor will ask you to choose between the recommended XDG autostart mode and the advanced systemd --user fallback mode, then explain when to pick each one.",
        conflicts_with = "disable_startup"
    )]
    pub enable_startup: bool,

    #[arg(
        long,
        help_heading = "Startup",
        help = "Disable automatic startup for the current user session.",
        long_help = "Removes Life Monitor collector startup for the current user.\n\nOn Windows, this removes the Startup shortcut.\nOn Linux, this removes the XDG autostart entry and disables/removes the optional systemd --user fallback unit.",
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

#[derive(Debug, Clone, Args, Default)]
#[command(about = "Open the interactive read-only dashboard backed by the local SQLite database.")]
pub struct DashboardCli {}

#[cfg(feature = "multi-sync")]
#[derive(Debug, Clone, Args, Default)]
pub struct SyncCli {
    #[arg(
        long,
        help_heading = "Database",
        value_name = "PATH",
        help = "Read the local SQLite database from a custom path."
    )]
    pub db_path: Option<PathBuf>,

    #[arg(
        long,
        help_heading = "Sync",
        value_name = "URL",
        help = "Remote sqld/libSQL endpoint used as the canonical sync store."
    )]
    pub sync_remote_url: Option<String>,

    #[arg(
        long,
        help_heading = "Sync",
        value_name = "TOKEN",
        help = "Authentication token for the remote sqld/libSQL endpoint."
    )]
    pub sync_auth_token: Option<String>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Collector(CollectorCli),
    Dashboard(DashboardCli),
    #[cfg(feature = "multi-sync")]
    Sync {
        #[command(subcommand)]
        action: SyncCommand,
        #[command(flatten)]
        args: SyncCli,
    },
}

#[derive(Parser, Debug, Clone)]
#[command(name = "Life Monitor")]
#[command(subcommand_required = true, arg_required_else_help = true)]
#[command(about = "Track keyboard, mouse, and focused-window activity into a SQLite database.")]
#[command(
    long_about = "Life Monitor records keyboard, mouse, scroll, and focused-window activity into a local SQLite database.\n\nUse `collector` to run the background collector and maintenance commands.\nUse `dashboard` to inspect the database through the interactive terminal dashboard."
)]
#[command(
    after_long_help = "Examples:\n  life-monitor collector\n  life-monitor collector --debug --interval 10\n  life-monitor collector --db-path /mnt/shared/life-monitor/data.db\n  life-monitor collector --export-db ./snapshot.sqlite\n  life-monitor collector --import-db ./snapshot.sqlite --dry-run\n  life-monitor dashboard"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

pub fn parse_cli() -> Cli {
    let mut command = Cli::command().styles(cli_styles());
    let matches = command.get_matches_mut();
    Cli::from_arg_matches(&matches).unwrap_or_else(|err| err.exit())
}

fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Green.on_default() | Effects::BOLD)
        .usage(AnsiColor::Green.on_default() | Effects::BOLD)
        .literal(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Yellow.on_default())
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Red.on_default() | Effects::BOLD)
        .error(AnsiColor::Red.on_default() | Effects::BOLD)
}

impl CollectorCli {
    #[allow(dead_code)]
    pub fn print_args(&self) {
        info!("Collector arguments provided:");
        info!("Interval: {:?}", self.interval.unwrap_or(300));
        #[cfg(target_os = "windows")]
        info!("No systray: {:?}", self.no_systray);
        info!("Debug mode: {:?}", self.debug);
        info!("Database path: {:?}", self.db_path);
        info!("Export database: {:?}", self.export_db);
        info!("Import database: {:?}", self.import_db);
        info!("Dry-run import: {:?}", self.dry_run);
        info!("Mouse DPI: {:?}", self.dpi.unwrap_or(DEFAULT_MOUSE_DPI));
        info!("Clear database: {:?}", self.clear);
        info!("Enable startup: {:?}", self.enable_startup);
        info!("Disable startup: {:?}", self.disable_startup);
        #[cfg(feature = "multi-sync")]
        info!("Sync enabled: {:?}", self.sync_enable);
        #[cfg(feature = "multi-sync")]
        info!("Sync remote URL: {:?}", self.sync_remote_url);
        println!();
    }
}
