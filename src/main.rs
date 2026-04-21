#[cfg(target_os = "windows")]
use crate::platform::windows::startup::configure_startup;

#[cfg(target_os = "windows")]
use crate::platform::windows::process;

#[cfg(target_os = "windows")]
use crate::platform::windows::systray;

#[cfg(target_os = "linux")]
use crate::platform::linux::common::*;

#[cfg(target_os = "linux")]
use crate::platform::linux::process;

use crate::storage::backend::*;
use crate::storage::localdb::{export_database, import_snapshot, plan_import, DbConfig};
#[cfg(feature = "multi-sync")]
use crate::storage::localdb::{open_con_at, setup_database};
#[cfg(feature = "multi-sync")]
use crate::sync::{
    record_sync_error, render_sync_status, resolve_sync_runtime_config, run_sync_cycle, sync_pull,
    sync_push, sync_status_snapshot, SqldRemote,
};
use crate::tui::run_dashboard;
use crate::utils::args::{parse_cli, Cli, CollectorCli, Command, DashboardCli};
#[cfg(feature = "multi-sync")]
use crate::utils::args::{SyncCli, SyncCommand};
use crate::utils::dpi::{log_mouse_dpi_resolution, resolve_mouse_dpi};
use crate::utils::lock::*;
use crate::utils::logger;

use anyhow::{Context, Result};

use tokio::task::JoinSet;
use tracing::*;

#[cfg(target_os = "linux")]
mod input_bindings;

mod common;
mod platform;
mod storage;
#[cfg(feature = "multi-sync")]
mod sync;
mod tui;
mod utils;

#[tokio::main]
async fn main() {
    let args = parse_cli();
    logger::init(debug_enabled(&args));
    logger::setup_panic_hook();

    if let Err(err) = run(args).await {
        error!("Fatal Error: {err:?}");
    }
}

async fn run(args: Cli) -> Result<()> {
    match args.command {
        Command::Collector(args) => run_collector(args).await,
        Command::Dashboard(args) => run_dashboard_mode(args).await,
        #[cfg(feature = "multi-sync")]
        Command::Sync { action, args } => run_sync_command(action, args).await,
    }
}

fn debug_enabled(cli: &Cli) -> bool {
    match &cli.command {
        Command::Collector(args) => args.debug,
        Command::Dashboard(_) => false,
        #[cfg(feature = "multi-sync")]
        Command::Sync { .. } => false,
    }
}

async fn run_dashboard_mode(_args: DashboardCli) -> Result<()> {
    let db_config = DbConfig::from_cli_path(None)?;
    run_dashboard(&db_config.db_path).with_context(|| "Failed to run terminal dashboard")
}

#[cfg(feature = "multi-sync")]
async fn run_sync_command(action: SyncCommand, args: SyncCli) -> Result<()> {
    let db_config = DbConfig::from_cli_path(args.db_path.clone())?;
    let mut conn = open_con_at(&db_config.db_path)?;
    setup_database(&conn)?;
    let sync_config = resolve_sync_runtime_config(
        &conn,
        args.sync_remote_url.as_deref(),
        args.sync_auth_token.as_deref(),
        false,
        300,
    )?
    .with_context(|| {
        "Sync is not configured. Pass --sync-remote-url or set LIFE_MONITOR_SYNC_REMOTE_URL."
    })?;
    let remote = SqldRemote::new(&sync_config.remote_url, &sync_config.auth_token).await?;

    match action {
        SyncCommand::Push => {
            sync_push(&conn, &remote, &sync_config).await?;
        }
        SyncCommand::Pull => {
            sync_pull(&mut conn, &remote, &sync_config).await?;
        }
        SyncCommand::Status => {
            let status = sync_status_snapshot(&conn, Some(&remote), &sync_config).await?;
            println!("{}", render_sync_status(&status));
        }
    }
    Ok(())
}

async fn run_collector(mut args: CollectorCli) -> Result<()> {
    let db_config = DbConfig::from_cli_path(args.db_path.clone())?;

    if args.enable_startup || args.disable_startup {
        let state = if args.enable_startup {
            "enable"
        } else {
            "disable"
        };

        configure_startup(&args).with_context(|| format!("Failed to {} startup", state))?;

        info!(
            "Startup {}d successfully. This command only installs or removes the startup entry and then exits. If you want to keep collecting in this session, run `life-monitor collector` again without the startup flags.",
            state
        );
        return Ok(());
    }

    if let Some(ref export_path) = args.export_db {
        let export = export_database(&db_config.db_path, export_path)
            .with_context(|| "Failed to export sqlite snapshot")?;
        info!(
            "Exported database snapshot to {} with export UUID {}",
            export.export_path.display(),
            export.export_uuid
        );
        return Ok(());
    }

    if let Some(ref import_path) = args.import_db {
        if args.dry_run {
            let plan = plan_import(&db_config.db_path, import_path)
                .with_context(|| "Failed to prepare import dry-run plan")?;
            println!("{}", plan.render());
            return Ok(());
        }

        let result = import_snapshot(
            &db_config.db_path,
            import_path,
            args.import_notes.as_deref(),
        )
        .with_context(|| "Failed to import sqlite snapshot")?;
        info!(
            "Imported snapshot. Automatic backup created at {}",
            result.destination_backup_path.display()
        );
        return Ok(());
    }

    ensure_single_instance()
        .with_context(|| "Failed to ensure that we are the only instance of the program")?;

    info!(
        "Lock acquired. Running application with PID {}",
        std::process::id()
    );

    if args.debug && args.interval.is_none() {
        info!("Debug mode enabled but no interval was provided. Using default value of 5 seconds!");
        args.interval = 5.into();
    }

    let db_update_interval = args.interval.unwrap_or(300);
    let mouse_dpi = resolve_mouse_dpi(args.dpi)?;
    log_mouse_dpi_resolution(mouse_dpi);

    let storage_backend = StorageBackend::Local(
        LocalDb::new(db_config, args.clear)
            .with_context(|| "Failed to initialize SQLite backend")?,
    );

    let mut tasks_set = JoinSet::new();

    #[cfg(feature = "multi-sync")]
    {
        let StorageBackend::Local(local_db) = &storage_backend;
        let sync_config = {
            let conn = local_db.shared_connection();
            let conn = conn.lock().unwrap();
            resolve_sync_runtime_config(
                &conn,
                args.sync_remote_url.as_deref(),
                args.sync_auth_token.as_deref(),
                args.sync_enable,
                args.sync_interval,
            )?
        };
        if let Some(sync_config) = sync_config.filter(|config| config.sync_enabled) {
            let db_path = local_db.db_path().clone();
            let sync_config = sync_config.clone();
            tasks_set.spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(
                    sync_config.sync_interval_seconds,
                ));
                let mut remote = None;
                loop {
                    tick.tick().await;
                    let _op_lock = acquire_db_operation_lock(&db_path)?;
                    // Sync must stay opportunistic. When the remote is down we only record the
                    // failure in local sync state and retry later; collection keeps running.
                    if remote.is_none() {
                        match SqldRemote::new(&sync_config.remote_url, &sync_config.auth_token)
                            .await
                        {
                            Ok(connected) => {
                                info!("Connected background sync to {}", sync_config.remote_url);
                                remote = Some(connected);
                            }
                            Err(err) => {
                                error!("Sync remote unavailable: {err:#}");
                                let conn = open_con_at(&db_path)?;
                                record_sync_error(
                                    &conn,
                                    &sync_config.own_source_uuid,
                                    &sync_config.remote_url,
                                    &err.to_string(),
                                )?;
                                continue;
                            }
                        }
                    }

                    if let Some(connected) = remote.as_ref() {
                        if let Err(err) = run_sync_cycle(&db_path, connected, &sync_config).await {
                            error!("Sync cycle failed: {err:#}");
                            remote = None;
                        }
                    }
                }
                #[allow(unreachable_code)]
                Ok::<(), anyhow::Error>(())
            });
        }
    }
    #[cfg(target_os = "linux")]
    tasks_set.spawn(crate::platform::linux::inputs::run(
        Some(mouse_dpi.dpi),
        db_update_interval + 5,
        storage_backend.clone(),
    ));

    #[cfg(target_os = "windows")]
    tasks_set.spawn(crate::platform::windows::inputs::run(
        Some(mouse_dpi.dpi),
        db_update_interval + 5,
        storage_backend.clone(),
    ));

    tasks_set.spawn(process::run(db_update_interval, storage_backend));

    #[cfg(target_os = "windows")]
    if !args.no_systray {
        tasks_set.spawn(systray::init_tray());
    }

    // Need to wait the tasks finish, which they shouldn't.
    while let Some(res) = tasks_set.join_next().await {
        // -> Option(Result(Result())))
        match res {
            Ok(Ok(())) => error!("Task exited cleanly but unexpectedly"),
            Ok(Err(err)) => return Err(err).with_context(|| "Task returned an error"),
            Err(join_err) => {
                return Err(anyhow::Error::new(join_err)).context("Task panicked or was cancelled")
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{FocusBucketRecord, InputBucketRecord, DEFAULT_SOURCE_ID};
    use crate::storage::localdb::{
        insert_focus_buckets, insert_input_buckets, open_con_at, setup_database,
    };
    use chrono::{TimeZone, Utc};
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use uuid::Uuid;

    fn unique_temp_db(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("life-monitor-main-{name}-{}.db", Uuid::new_v4()))
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("life-monitor-main-{name}-{}", Uuid::new_v4()))
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn base_collector_args() -> CollectorCli {
        CollectorCli {
            interval: None,
            #[cfg(target_os = "windows")]
            no_systray: true,
            debug: false,
            db_path: None,
            export_db: None,
            import_db: None,
            dry_run: false,
            import_notes: None,
            dpi: None,
            clear: false,
            enable_startup: false,
            disable_startup: false,
            #[cfg(feature = "multi-sync")]
            sync_enable: false,
            #[cfg(feature = "multi-sync")]
            sync_remote_url: None,
            #[cfg(feature = "multi-sync")]
            sync_auth_token: None,
            #[cfg(feature = "multi-sync")]
            sync_interval: 300,
        }
    }

    fn sample_input_row() -> InputBucketRecord {
        InputBucketRecord {
            source_id: DEFAULT_SOURCE_ID,
            bucket_start_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap(),
            bucket_end_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 15, 0).unwrap(),
            local_date: "2026-04-18".to_string(),
            local_hour: 9,
            timezone_offset_minutes: -180,
            granularity_minutes: 15,
            left_clicks: 2,
            right_clicks: 1,
            middle_clicks: 0,
            key_presses: 5,
            mouse_distance_cm: 3.0,
            scroll_vertical_cm: 0.4,
            scroll_horizontal_cm: 0.0,
        }
    }

    fn sample_focus_row() -> FocusBucketRecord {
        FocusBucketRecord {
            source_id: DEFAULT_SOURCE_ID,
            bucket_start_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap(),
            bucket_end_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 15, 0).unwrap(),
            local_date: "2026-04-18".to_string(),
            local_hour: 9,
            timezone_offset_minutes: -180,
            app_identifier: "firefox".to_string(),
            window_title: "Docs".to_string(),
            window_class: "firefox".to_string(),
            focus_seconds: 120,
        }
    }

    /// Verifies that the `--export-db` command path exits after writing a snapshot by
    /// seeding a real temporary database, running `run`, and checking the snapshot contents.
    #[tokio::test]
    async fn run_export_db_creates_snapshot_and_exits() -> Result<()> {
        let _guard = env_lock().lock().unwrap();
        let data_dir = unique_temp_dir("data-dir");
        std::fs::create_dir_all(&data_dir)?;
        std::env::set_var("LIFE_MONITOR_SKIP_INSTANCE_LOCK", "1");
        std::env::set_var("LIFE_MONITOR_DATA_DIR", &data_dir);
        let db_path = unique_temp_db("export-source");
        let export_path = unique_temp_db("export-out");
        let conn = open_con_at(&db_path)?;
        setup_database(&conn)?;
        insert_input_buckets(&conn, &[sample_input_row()])?;

        let mut args = base_collector_args();
        args.db_path = Some(db_path.clone());
        args.export_db = Some(export_path.clone());
        run(Cli {
            command: Command::Collector(args),
        })
        .await?;

        let snapshot = open_con_at(&export_path)?;
        let count: u64 =
            snapshot.query_row("SELECT COUNT(*) FROM input_buckets", [], |row| row.get(0))?;
        assert_eq!(count, 1);

        drop(snapshot);
        drop(conn);
        std::fs::remove_file(db_path)?;
        std::fs::remove_file(export_path)?;
        std::env::remove_var("LIFE_MONITOR_SKIP_INSTANCE_LOCK");
        std::env::remove_var("LIFE_MONITOR_DATA_DIR");
        std::fs::remove_dir_all(data_dir)?;
        Ok(())
    }

    /// Verifies that `--import-db --dry-run` leaves the destination unchanged by comparing
    /// imports metadata and bucket totals before and after running the CLI short-circuit path.
    #[tokio::test]
    async fn run_import_db_dry_run_leaves_destination_unchanged() -> Result<()> {
        let _guard = env_lock().lock().unwrap();
        let data_dir = unique_temp_dir("data-dir");
        std::fs::create_dir_all(&data_dir)?;
        std::env::set_var("LIFE_MONITOR_SKIP_INSTANCE_LOCK", "1");
        std::env::set_var("LIFE_MONITOR_DATA_DIR", &data_dir);
        let dest_path = unique_temp_db("dry-run-dest");
        let source_path = unique_temp_db("dry-run-source");
        let export_path = unique_temp_db("dry-run-export");

        let dest = open_con_at(&dest_path)?;
        setup_database(&dest)?;
        insert_input_buckets(&dest, &[sample_input_row()])?;

        let source = open_con_at(&source_path)?;
        setup_database(&source)?;
        insert_input_buckets(
            &source,
            &[InputBucketRecord {
                key_presses: 9,
                ..sample_input_row()
            }],
        )?;
        crate::storage::localdb::export_database(&source_path, &export_path)?;

        let mut args = base_collector_args();
        args.db_path = Some(dest_path.clone());
        args.import_db = Some(export_path.clone());
        args.dry_run = true;
        run(Cli {
            command: Command::Collector(args),
        })
        .await?;

        let after = open_con_at(&dest_path)?;
        let imports_count: u64 =
            after.query_row("SELECT COUNT(*) FROM imports", [], |row| row.get(0))?;
        let key_presses: u64 =
            after.query_row("SELECT key_presses FROM input_buckets", [], |row| {
                row.get(0)
            })?;
        assert_eq!(imports_count, 0);
        assert_eq!(key_presses, 5);

        drop(after);
        drop(source);
        drop(dest);
        std::fs::remove_file(dest_path)?;
        std::fs::remove_file(source_path)?;
        std::fs::remove_file(export_path)?;
        std::env::remove_var("LIFE_MONITOR_SKIP_INSTANCE_LOCK");
        std::env::remove_var("LIFE_MONITOR_DATA_DIR");
        std::fs::remove_dir_all(data_dir)?;
        Ok(())
    }

    /// Verifies that the import CLI path merges into the configured custom database by using
    /// real exported data, then asserting the destination gained merged focus and import rows.
    #[tokio::test]
    async fn run_import_db_merges_snapshot_using_custom_db_path() -> Result<()> {
        let _guard = env_lock().lock().unwrap();
        let data_dir = unique_temp_dir("data-dir");
        std::fs::create_dir_all(&data_dir)?;
        std::env::set_var("LIFE_MONITOR_SKIP_INSTANCE_LOCK", "1");
        std::env::set_var("LIFE_MONITOR_DATA_DIR", &data_dir);
        let dest_path = unique_temp_db("custom-dest");
        let source_path = unique_temp_db("custom-source");
        let export_path = unique_temp_db("custom-export");

        let dest = open_con_at(&dest_path)?;
        setup_database(&dest)?;
        insert_input_buckets(&dest, &[sample_input_row()])?;

        let source = open_con_at(&source_path)?;
        setup_database(&source)?;
        let dest_uuid: String = dest.query_row(
            "SELECT source_uuid FROM sources WHERE id = ?1",
            [DEFAULT_SOURCE_ID],
            |row| row.get(0),
        )?;
        source.execute(
            "UPDATE sources SET source_uuid = ?1 WHERE id = ?2",
            rusqlite::params![dest_uuid, DEFAULT_SOURCE_ID],
        )?;
        insert_input_buckets(
            &source,
            &[InputBucketRecord {
                key_presses: 9,
                ..sample_input_row()
            }],
        )?;
        insert_focus_buckets(&source, &[sample_focus_row()])?;
        crate::storage::localdb::export_database(&source_path, &export_path)?;

        let mut args = base_collector_args();
        args.db_path = Some(dest_path.clone());
        args.import_db = Some(export_path.clone());
        args.import_notes = Some("cli import".to_string());
        run(Cli {
            command: Command::Collector(args),
        })
        .await?;

        let after = open_con_at(&dest_path)?;
        let imports_count: u64 =
            after.query_row("SELECT COUNT(*) FROM imports", [], |row| row.get(0))?;
        let focus_count: u64 =
            after.query_row("SELECT COUNT(*) FROM focus_buckets", [], |row| row.get(0))?;
        assert_eq!(imports_count, 1);
        assert_eq!(focus_count, 1);

        drop(after);
        drop(source);
        drop(dest);
        std::fs::remove_file(dest_path)?;
        std::fs::remove_file(source_path)?;
        std::fs::remove_file(export_path)?;
        let temp_dir = std::env::temp_dir();
        for entry in std::fs::read_dir(temp_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.contains("custom-dest") && name.contains("pre-import") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        std::env::remove_var("LIFE_MONITOR_SKIP_INSTANCE_LOCK");
        std::env::remove_var("LIFE_MONITOR_DATA_DIR");
        std::fs::remove_dir_all(data_dir)?;
        Ok(())
    }
}
