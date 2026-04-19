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
use crate::storage::localdb::{
    app_activity_report, export_database, import_snapshot, open_con_at, plan_import,
    session_report, setup_database, DbConfig,
};
use crate::utils::args::{Cli, ReportKind};
use crate::utils::dpi::{log_mouse_dpi_resolution, resolve_mouse_dpi};
use crate::utils::lock::*;
use crate::utils::logger;

use anyhow::{Context, Result};
use clap::Parser;

use tokio::task::JoinSet;
use tracing::*;

#[cfg(target_os = "linux")]
mod input_bindings;

mod common;
mod platform;
mod storage;
mod utils;

#[tokio::main]
async fn main() {
    let args = Cli::parse();
    logger::init(args.debug);
    logger::setup_panic_hook();

    if let Err(err) = run(args).await {
        error!("Fatal Error: {err:?}");
    }
}

async fn run(mut args: Cli) -> Result<()> {
    let db_config = DbConfig::from_cli_path(args.db_path.clone())?;

    // if we receive one of these two flags we call the function and it will enable or disable the
    // startup depending on the enable value.
    if args.enable_startup || args.disable_startup {
        let state = if args.enable_startup {
            "enable"
        } else {
            "disable"
        };

        configure_startup(&args).with_context(|| format!("Failed to {} startup", state))?;

        info!("Startup {}d successfully, the program will end now. Start it again without the start up flag to run normally.",state);
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

    if let Some(report) = args.report {
        let conn = open_con_at(&db_config.db_path)?;
        setup_database(&conn)?;
        match report {
            ReportKind::Sessions => {
                for row in session_report(&conn, args.report_days)? {
                    let ended = row.ended_at_utc.as_deref().unwrap_or("running");
                    let duration = row
                        .duration_seconds
                        .map(|value| format!("{value}s"))
                        .unwrap_or_else(|| "unknown".to_string());
                    println!(
                        "{} {} {} {}",
                        row.started_at_utc, ended, duration, row.platform
                    );
                }
            }
            ReportKind::Apps => {
                for row in app_activity_report(&conn, args.report_days)? {
                    println!("{} {}", row.focus_seconds, row.app_identifier);
                }
            }
        }
        return Ok(());
    }

    let db_update_interval = args.interval.unwrap_or(300);
    let mouse_dpi = resolve_mouse_dpi(args.dpi)?;
    log_mouse_dpi_resolution(mouse_dpi);

    let storage_backend = StorageBackend::Local(
        LocalDb::new(db_config, args.clear)
            .with_context(|| "Failed to initialize SQLite backend")?,
    );

    let mut tasks_set = JoinSet::new();
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

    fn base_cli() -> Cli {
        Cli {
            interval: None,
            #[cfg(target_os = "windows")]
            no_systray: true,
            debug: false,
            db_path: None,
            export_db: None,
            import_db: None,
            dry_run: false,
            import_notes: None,
            report: None,
            report_days: 7,
            dpi: None,
            clear: false,
            enable_startup: false,
            disable_startup: false,
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

        let mut cli = base_cli();
        cli.db_path = Some(db_path.clone());
        cli.export_db = Some(export_path.clone());
        run(cli).await?;

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

        let mut cli = base_cli();
        cli.db_path = Some(dest_path.clone());
        cli.import_db = Some(export_path.clone());
        cli.dry_run = true;
        run(cli).await?;

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

        let mut cli = base_cli();
        cli.db_path = Some(dest_path.clone());
        cli.import_db = Some(export_path.clone());
        cli.import_notes = Some("cli import".to_string());
        run(cli).await?;

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
