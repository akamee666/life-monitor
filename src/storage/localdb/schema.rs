use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::io;
use std::path::Path;
use tracing::{info, warn};
use uuid::Uuid;

use crate::common::DEFAULT_SOURCE_ID;

pub const SCHEMA_VERSION: i64 = 3;

pub fn setup_database(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS schema_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sources (
            id INTEGER PRIMARY KEY,
            source_uuid TEXT NOT NULL UNIQUE,
            source_name TEXT NOT NULL,
            platform TEXT NOT NULL,
            created_at_utc TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS input_buckets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id INTEGER NOT NULL,
            bucket_start_utc TEXT NOT NULL,
            bucket_end_utc TEXT NOT NULL,
            local_date TEXT NOT NULL,
            local_hour INTEGER NOT NULL,
            timezone_offset_minutes INTEGER NOT NULL,
            granularity_minutes INTEGER NOT NULL,
            left_clicks INTEGER NOT NULL,
            right_clicks INTEGER NOT NULL,
            middle_clicks INTEGER NOT NULL,
            key_presses INTEGER NOT NULL,
            mouse_distance_cm REAL NOT NULL,
            scroll_vertical_cm REAL NOT NULL,
            scroll_horizontal_cm REAL NOT NULL,
            FOREIGN KEY(source_id) REFERENCES sources(id),
            UNIQUE(source_id, bucket_start_utc, granularity_minutes)
        );

        CREATE TABLE IF NOT EXISTS focus_buckets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id INTEGER NOT NULL,
            bucket_start_utc TEXT NOT NULL,
            bucket_end_utc TEXT NOT NULL,
            local_date TEXT NOT NULL,
            local_hour INTEGER NOT NULL,
            timezone_offset_minutes INTEGER NOT NULL,
            app_identifier TEXT NOT NULL,
            window_title TEXT NOT NULL,
            window_class TEXT NOT NULL,
            focus_seconds INTEGER NOT NULL,
            FOREIGN KEY(source_id) REFERENCES sources(id),
            UNIQUE(source_id, bucket_start_utc, window_title, window_class)
        );

        CREATE TABLE IF NOT EXISTS exports (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            export_uuid TEXT NOT NULL UNIQUE,
            primary_source_uuid TEXT NOT NULL,
            exported_at_utc TEXT NOT NULL,
            schema_version INTEGER NOT NULL,
            notes TEXT
        );

        CREATE TABLE IF NOT EXISTS imports (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            import_uuid TEXT NOT NULL UNIQUE,
            source_export_uuid TEXT NOT NULL UNIQUE,
            source_source_uuid TEXT NOT NULL,
            exported_at_utc TEXT NOT NULL,
            imported_at_utc TEXT NOT NULL,
            file_hash TEXT NOT NULL,
            schema_version INTEGER NOT NULL,
            notes TEXT
        );

        CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id INTEGER NOT NULL,
            started_at_utc TEXT NOT NULL,
            ended_at_utc TEXT,
            session_uuid TEXT NOT NULL UNIQUE,
            platform TEXT NOT NULL,
            FOREIGN KEY(source_id) REFERENCES sources(id)
        );

        CREATE TABLE IF NOT EXISTS sync_state (
            own_source_uuid TEXT NOT NULL,
            remote_url TEXT NOT NULL,
            last_pulled_revision INTEGER NOT NULL DEFAULT 0,
            last_pushed_batch_uuid TEXT,
            last_push_at_utc TEXT,
            last_pull_at_utc TEXT,
            last_sync_error TEXT,
            last_sync_error_at_utc TEXT,
            remote_head_revision INTEGER,
            sync_enabled INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (own_source_uuid, remote_url)
        );

        CREATE TABLE IF NOT EXISTS sync_outbox_sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_uuid TEXT,
            source_uuid TEXT NOT NULL,
            source_name TEXT NOT NULL,
            platform TEXT NOT NULL,
            created_at_utc TEXT NOT NULL,
            sent_at_utc TEXT,
            attempt_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE UNIQUE INDEX IF NOT EXISTS sync_outbox_sources_pending_idx
        ON sync_outbox_sources (source_uuid)
        WHERE sent_at_utc IS NULL;

        CREATE TABLE IF NOT EXISTS sync_outbox_input_buckets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_uuid TEXT,
            source_uuid TEXT NOT NULL,
            bucket_start_utc TEXT NOT NULL,
            bucket_end_utc TEXT NOT NULL,
            local_date TEXT NOT NULL,
            local_hour INTEGER NOT NULL,
            timezone_offset_minutes INTEGER NOT NULL,
            granularity_minutes INTEGER NOT NULL,
            left_clicks INTEGER NOT NULL,
            right_clicks INTEGER NOT NULL,
            middle_clicks INTEGER NOT NULL,
            key_presses INTEGER NOT NULL,
            mouse_distance_cm REAL NOT NULL,
            scroll_vertical_cm REAL NOT NULL,
            scroll_horizontal_cm REAL NOT NULL,
            created_at_utc TEXT NOT NULL,
            sent_at_utc TEXT,
            attempt_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE UNIQUE INDEX IF NOT EXISTS sync_outbox_input_pending_idx
        ON sync_outbox_input_buckets (source_uuid, bucket_start_utc, granularity_minutes)
        WHERE sent_at_utc IS NULL;

        CREATE TABLE IF NOT EXISTS sync_outbox_focus_buckets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_uuid TEXT,
            source_uuid TEXT NOT NULL,
            bucket_start_utc TEXT NOT NULL,
            bucket_end_utc TEXT NOT NULL,
            local_date TEXT NOT NULL,
            local_hour INTEGER NOT NULL,
            timezone_offset_minutes INTEGER NOT NULL,
            app_identifier TEXT NOT NULL,
            window_title TEXT NOT NULL,
            window_class TEXT NOT NULL,
            focus_seconds INTEGER NOT NULL,
            created_at_utc TEXT NOT NULL,
            sent_at_utc TEXT,
            attempt_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE UNIQUE INDEX IF NOT EXISTS sync_outbox_focus_pending_idx
        ON sync_outbox_focus_buckets (source_uuid, bucket_start_utc, window_title, window_class)
        WHERE sent_at_utc IS NULL;
        ",
    )
    .with_context(|| "Failed to initialize local sqlite schema")?;

    conn.execute(
        "
        INSERT INTO schema_meta (key, value)
        VALUES ('schema_version', ?1)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        ",
        [SCHEMA_VERSION.to_string()],
    )
    .with_context(|| "Failed to store schema version metadata")?;

    ensure_default_source(conn)?;
    Ok(())
}

fn ensure_default_source(conn: &Connection) -> Result<()> {
    let existing_uuid: Option<String> = conn
        .query_row(
            "SELECT source_uuid FROM sources WHERE id = ?1",
            [DEFAULT_SOURCE_ID],
            |row| row.get(0),
        )
        .optional()?;

    let source_uuid = existing_uuid.unwrap_or_else(|| Uuid::new_v4().to_string());
    conn.execute(
        "
        INSERT INTO sources (id, source_uuid, source_name, platform, created_at_utc)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(id) DO UPDATE SET
            source_uuid = excluded.source_uuid,
            source_name = excluded.source_name,
            platform = excluded.platform
        ",
        params![
            DEFAULT_SOURCE_ID,
            source_uuid,
            default_source_name(),
            default_source_platform(),
            Utc::now().to_rfc3339(),
        ],
    )
    .with_context(|| "Failed to ensure the default source row exists")?;

    Ok(())
}

pub fn clear_database(db_path: &Path) -> Result<()> {
    match fs::remove_file(db_path) {
        Ok(_) => {
            info!(
                "Successfully removed database file: '{}'",
                db_path.display()
            );
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            warn!(
                "Skipping cleanup: sqlite database not found at: '{}'",
                db_path.display()
            );
            Ok(())
        }
        Err(err) => Err(err).with_context(|| {
            format!(
                "Failed to delete sqlite database at: '{}'",
                db_path.display()
            )
        }),
    }
}

fn default_source_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "default".to_string())
}

fn default_source_platform() -> String {
    if cfg!(target_os = "windows") {
        "windows".to_string()
    } else if cfg!(target_os = "linux") {
        "linux".to_string()
    } else {
        std::env::consts::OS.to_string()
    }
}
