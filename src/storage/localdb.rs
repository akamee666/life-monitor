use crate::common::*;
use crate::utils::lock::acquire_db_operation_lock;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use rusqlite::backup;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

use tracing::*;

pub const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct ExportMetadata {
    pub export_uuid: String,
    pub primary_source_uuid: String,
    pub exported_at_utc: String,
    pub schema_version: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportResult {
    pub export_path: PathBuf,
    pub export_uuid: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportPlan {
    pub source_export_uuid: String,
    pub source_file_hash: String,
    pub duplicate_import: bool,
    pub duplicate_reason: Option<String>,
    pub new_sources: u64,
    pub new_input_buckets: u64,
    pub updated_input_buckets: u64,
    pub input_key_presses_delta: u64,
    pub input_left_clicks_delta: u64,
    pub input_right_clicks_delta: u64,
    pub input_middle_clicks_delta: u64,
    pub input_mouse_distance_cm_delta: f64,
    pub new_focus_buckets: u64,
    pub updated_focus_buckets: u64,
    pub focus_seconds_delta: u64,
}

impl ImportPlan {
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("import snapshot {}", self.source_export_uuid));

        if self.duplicate_import {
            lines.push(format!(
                "! duplicate import detected: {}",
                self.duplicate_reason
                    .as_deref()
                    .unwrap_or("this snapshot was already imported")
            ));
        }

        lines.push(format!("+ sources: {} new", self.new_sources));
        lines.push(format!("+ input buckets: {} new", self.new_input_buckets));
        lines.push(format!(
            "~ input buckets: {} existing rows will be incremented",
            self.updated_input_buckets
        ));
        lines.push(format!(
            "~ input totals: key_presses +{}, left_clicks +{}, right_clicks +{}, middle_clicks +{}, mouse_distance_cm +{:.2}",
            self.input_key_presses_delta,
            self.input_left_clicks_delta,
            self.input_right_clicks_delta,
            self.input_middle_clicks_delta,
            self.input_mouse_distance_cm_delta
        ));
        lines.push(format!("+ focus buckets: {} new", self.new_focus_buckets));
        lines.push(format!(
            "~ focus buckets: {} existing rows will be incremented",
            self.updated_focus_buckets
        ));
        lines.push(format!(
            "~ focus totals: focus_seconds +{}",
            self.focus_seconds_delta
        ));
        lines.join("\n")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportResult {
    pub destination_backup_path: PathBuf,
    pub plan: ImportPlan,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DbConfig {
    pub db_path: PathBuf,
    pub source: DbPathSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbPathSource {
    Default,
    Remembered,
    Cli,
}

impl DbConfig {
    pub fn from_cli_path(path: Option<PathBuf>) -> Result<Self> {
        if let Some(path) = path {
            let db_path = resolve_db_path(Some(path.as_path()))?;
            store_remembered_db_path(&db_path)?;
            info!(
                "Using database path from --db-path: {}. This path is now remembered for future runs.",
                db_path.display()
            );
            return Ok(Self {
                db_path,
                source: DbPathSource::Cli,
            });
        }

        if let Some(path) = load_remembered_db_path()? {
            let db_path = resolve_db_path(Some(path.as_path())).with_context(|| {
                format_remembered_path_error(
                    &path,
                    "the remembered database path could not be prepared",
                )
            })?;
            info!("Using remembered database path: {}", db_path.display());
            return Ok(Self {
                db_path,
                source: DbPathSource::Remembered,
            });
        }

        let db_path = resolve_db_path(None)?;
        info!("Using default database path: {}", db_path.display());
        Ok(Self {
            db_path,
            source: DbPathSource::Default,
        })
    }
}

pub fn resolve_db_path(custom_path: Option<&Path>) -> Result<PathBuf> {
    let path = match custom_path {
        Some(path) => normalize_custom_db_path(path)?,
        None => default_db_path()?,
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for sqlite database: {}",
                parent.display()
            )
        })?;
    }

    Ok(path)
}

fn normalize_custom_db_path(path: &Path) -> Result<PathBuf> {
    if path.is_dir() {
        let is_empty = fs::read_dir(path)
            .with_context(|| format!("Failed to inspect database directory '{}'", path.display()))?
            .next()
            .is_none();
        let db_path = path.join("data.db");

        if is_empty {
            info!(
                "--db-path pointed to the empty directory '{}'. Life Monitor will create a new SQLite database at '{}'.",
                path.display(),
                db_path.display()
            );
        } else if db_path.exists() {
            info!(
                "--db-path pointed to the directory '{}'. Using the SQLite database file '{}'.",
                path.display(),
                db_path.display()
            );
        } else {
            info!(
                "--db-path pointed to the directory '{}'. Life Monitor will create a SQLite database file at '{}'.",
                path.display(),
                db_path.display()
            );
        }

        return Ok(db_path);
    }

    if !path.exists() && looks_like_directory_path(path) {
        fs::create_dir_all(path).with_context(|| {
            format!(
                "Failed to create the database directory provided through --db-path: '{}'",
                path.display()
            )
        })?;
        let db_path = path.join("data.db");
        info!(
            "--db-path looked like a directory path that did not exist yet. Life Monitor created '{}' and will use '{}'.",
            path.display(),
            db_path.display()
        );
        return Ok(db_path);
    }

    Ok(path.to_path_buf())
}

fn looks_like_directory_path(path: &Path) -> bool {
    path.extension().is_none()
}

pub fn default_db_path() -> Result<PathBuf> {
    Ok(program_data_dir()
        .with_context(|| "Could not determine the default data directory")?
        .join("data.db"))
}

fn remembered_db_path_file() -> Result<PathBuf> {
    Ok(program_data_dir()
        .with_context(|| "Could not determine the application data directory for DB path memory")?
        .join("last-db-path.txt"))
}

fn load_remembered_db_path() -> Result<Option<PathBuf>> {
    let path_file = remembered_db_path_file()?;
    match fs::read_to_string(&path_file) {
        Ok(contents) => {
            let trimmed = contents.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(PathBuf::from(trimmed)))
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| {
            format!(
                "Failed to read the remembered database path from '{}'",
                path_file.display()
            )
        }),
    }
}

fn store_remembered_db_path(path: &Path) -> Result<()> {
    let path_file = remembered_db_path_file()?;
    fs::write(&path_file, path.to_string_lossy().as_bytes()).with_context(|| {
        format!(
            "Failed to remember the last database path in '{}'",
            path_file.display()
        )
    })
}

fn format_remembered_path_error(path: &Path, reason: &str) -> String {
    format!(
        "{reason}.\nRemembered database path: '{}'\nWhat you can do:\n- mount or reconnect the share again so Life Monitor can access it\n- run Life Monitor with --db-path <NEW_PATH> to use another database location now\n- if needed, import old data later once the original database or a snapshot is available",
        path.display()
    )
}

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

pub fn insert_input_buckets(conn: &Connection, rows: &[InputBucketRecord]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut stmt = conn.prepare_cached(
        "
        INSERT INTO input_buckets (
            source_id,
            bucket_start_utc,
            bucket_end_utc,
            local_date,
            local_hour,
            timezone_offset_minutes,
            granularity_minutes,
            left_clicks,
            right_clicks,
            middle_clicks,
            key_presses,
            mouse_distance_cm,
            scroll_vertical_cm,
            scroll_horizontal_cm
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(source_id, bucket_start_utc, granularity_minutes) DO UPDATE SET
            bucket_end_utc = excluded.bucket_end_utc,
            local_date = excluded.local_date,
            local_hour = excluded.local_hour,
            timezone_offset_minutes = excluded.timezone_offset_minutes,
            left_clicks = input_buckets.left_clicks + excluded.left_clicks,
            right_clicks = input_buckets.right_clicks + excluded.right_clicks,
            middle_clicks = input_buckets.middle_clicks + excluded.middle_clicks,
            key_presses = input_buckets.key_presses + excluded.key_presses,
            mouse_distance_cm = input_buckets.mouse_distance_cm + excluded.mouse_distance_cm,
            scroll_vertical_cm = input_buckets.scroll_vertical_cm + excluded.scroll_vertical_cm,
            scroll_horizontal_cm = input_buckets.scroll_horizontal_cm + excluded.scroll_horizontal_cm
        ",
    )?;

    for row in rows {
        stmt.execute(params![
            row.source_id,
            row.bucket_start_utc.to_rfc3339(),
            row.bucket_end_utc.to_rfc3339(),
            row.local_date,
            row.local_hour,
            row.timezone_offset_minutes,
            row.granularity_minutes,
            row.left_clicks,
            row.right_clicks,
            row.middle_clicks,
            row.key_presses,
            row.mouse_distance_cm,
            row.scroll_vertical_cm,
            row.scroll_horizontal_cm,
        ])
        .with_context(|| "Failed to insert input bucket row")?;
    }

    Ok(())
}

pub fn insert_focus_buckets(conn: &Connection, rows: &[FocusBucketRecord]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut stmt = conn.prepare_cached(
        "
        INSERT INTO focus_buckets (
            source_id,
            bucket_start_utc,
            bucket_end_utc,
            local_date,
            local_hour,
            timezone_offset_minutes,
            app_identifier,
            window_title,
            window_class,
            focus_seconds
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(source_id, bucket_start_utc, window_title, window_class) DO UPDATE SET
            bucket_end_utc = excluded.bucket_end_utc,
            local_date = excluded.local_date,
            local_hour = excluded.local_hour,
            timezone_offset_minutes = excluded.timezone_offset_minutes,
            app_identifier = excluded.app_identifier,
            focus_seconds = focus_buckets.focus_seconds + excluded.focus_seconds
        ",
    )?;

    for row in rows {
        stmt.execute(params![
            row.source_id,
            row.bucket_start_utc.to_rfc3339(),
            row.bucket_end_utc.to_rfc3339(),
            row.local_date,
            row.local_hour,
            row.timezone_offset_minutes,
            row.app_identifier,
            row.window_title,
            row.window_class,
            row.focus_seconds,
        ])
        .with_context(|| "Failed to insert focus bucket row")?;
    }

    Ok(())
}

pub fn get_source(conn: &Connection, source_id: i64) -> Result<SourceInfo> {
    conn.query_row(
        "SELECT id, source_uuid, source_name, platform FROM sources WHERE id = ?1",
        [source_id],
        |row| {
            Ok(SourceInfo {
                id: row.get(0)?,
                source_uuid: row.get(1)?,
                source_name: row.get(2)?,
                platform: row.get(3)?,
            })
        },
    )
    .with_context(|| format!("Failed to retrieve source row with id {source_id}"))
}

pub fn open_con_at(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for sqlite database: {}",
                parent.display()
            )
        })?;
    }

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .with_context(|| {
        format!(
            "Failed to open database connection with sqlite database at: {}",
            db_path.display()
        )
    })?;
    conn.busy_timeout(Duration::from_secs(5))
        .with_context(|| "Failed to configure sqlite busy timeout")?;
    Ok(conn)
}

pub fn export_database(source_db_path: &Path, export_path: &Path) -> Result<ExportResult> {
    if export_path.exists() {
        bail!(
            "Refusing to overwrite existing export snapshot: {}",
            export_path.display()
        );
    }

    if let Some(parent) = export_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for export snapshot: {}",
                parent.display()
            )
        })?;
    }

    let _lock = acquire_db_operation_lock(source_db_path)?;
    info!(
        "Creating SQLite snapshot export from '{}' to '{}'.",
        source_db_path.display(),
        export_path.display()
    );
    let source = open_con_at(source_db_path)?;
    setup_database(&source)?;
    run_integrity_check(&source, "source database")?;
    backup_database_contents(&source, export_path)?;

    let snapshot = open_con_at(export_path)?;
    setup_database(&snapshot)?;

    let primary_source = get_source(&source, DEFAULT_SOURCE_ID)?;
    let export_uuid = Uuid::new_v4().to_string();
    snapshot.execute(
        "
        INSERT INTO exports (export_uuid, primary_source_uuid, exported_at_utc, schema_version, notes)
        VALUES (?1, ?2, ?3, ?4, NULL)
        ",
        params![
            export_uuid,
            primary_source.source_uuid,
            Utc::now().to_rfc3339(),
            SCHEMA_VERSION,
        ],
    )?;

    Ok(ExportResult {
        export_path: export_path.to_path_buf(),
        export_uuid,
    })
}

pub fn plan_import(destination_db_path: &Path, source_snapshot_path: &Path) -> Result<ImportPlan> {
    let _lock = acquire_db_operation_lock(destination_db_path)?;
    info!(
        "Planning snapshot import from '{}' into '{}'.",
        source_snapshot_path.display(),
        destination_db_path.display()
    );
    plan_import_locked(destination_db_path, source_snapshot_path)
}

fn plan_import_locked(
    destination_db_path: &Path,
    source_snapshot_path: &Path,
) -> Result<ImportPlan> {
    let destination = open_con_at(destination_db_path)?;
    setup_database(&destination)?;
    let source =
        Connection::open_with_flags(source_snapshot_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| {
                format!(
                    "Failed to open source snapshot for import planning: {}",
                    source_snapshot_path.display()
                )
            })?;

    run_integrity_check(&destination, "destination database")?;
    run_integrity_check(&source, "source snapshot")?;
    validate_schema_version(&destination, "destination database")?;
    validate_schema_version(&source, "source snapshot")?;

    let metadata = latest_export_metadata(&source)?;
    let file_hash = file_sha256(source_snapshot_path)?;
    let duplicate_reason = existing_import_reason(&destination, &metadata.export_uuid, &file_hash)?;

    attach_source(&destination, source_snapshot_path)?;
    let plan = query_import_plan(&destination, &metadata, &file_hash, duplicate_reason)?;
    detach_source(&destination)?;
    Ok(plan)
}

pub fn import_snapshot(
    destination_db_path: &Path,
    source_snapshot_path: &Path,
    notes: Option<&str>,
) -> Result<ImportResult> {
    let _lock = acquire_db_operation_lock(destination_db_path)?;
    info!(
        "Starting snapshot import from '{}' into '{}'.",
        source_snapshot_path.display(),
        destination_db_path.display()
    );
    let plan = plan_import_locked(destination_db_path, source_snapshot_path)?;
    if plan.duplicate_import {
        bail!(
            "Import refused because this snapshot was already imported: {}",
            plan.duplicate_reason
                .as_deref()
                .unwrap_or("duplicate snapshot")
        );
    }

    let backup_path = default_pre_import_backup_path(destination_db_path);
    info!(
        "Creating automatic pre-import backup at '{}'.",
        backup_path.display()
    );
    let destination_for_backup = open_con_at(destination_db_path)?;
    backup_database_contents(&destination_for_backup, &backup_path)
        .with_context(|| "Failed to create automatic destination backup before import")?;

    let destination = open_con_at(destination_db_path)?;
    attach_source(&destination, source_snapshot_path)?;
    let metadata = latest_export_metadata_from_attached(&destination)?;

    let tx = destination.unchecked_transaction()?;
    tx.execute_batch(
        "
        UPDATE main.sources
        SET
            source_name = (
                SELECT src.source_name
                FROM import_src.sources src
                WHERE src.source_uuid = main.sources.source_uuid
            ),
            platform = (
                SELECT src.platform
                FROM import_src.sources src
                WHERE src.source_uuid = main.sources.source_uuid
            )
        WHERE EXISTS (
            SELECT 1
            FROM import_src.sources src
            WHERE src.source_uuid = main.sources.source_uuid
        );

        INSERT OR IGNORE INTO main.sources (source_uuid, source_name, platform, created_at_utc)
        SELECT source_uuid, source_name, platform, created_at_utc
        FROM import_src.sources;

        UPDATE main.input_buckets
        SET
            bucket_end_utc = (
                SELECT ib.bucket_end_utc
                FROM import_src.input_buckets ib
                JOIN import_src.sources src_src ON src_src.id = ib.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.input_buckets.source_id
                  AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
                  AND ib.granularity_minutes = main.input_buckets.granularity_minutes
            ),
            local_date = (
                SELECT ib.local_date
                FROM import_src.input_buckets ib
                JOIN import_src.sources src_src ON src_src.id = ib.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.input_buckets.source_id
                  AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
                  AND ib.granularity_minutes = main.input_buckets.granularity_minutes
            ),
            local_hour = (
                SELECT ib.local_hour
                FROM import_src.input_buckets ib
                JOIN import_src.sources src_src ON src_src.id = ib.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.input_buckets.source_id
                  AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
                  AND ib.granularity_minutes = main.input_buckets.granularity_minutes
            ),
            timezone_offset_minutes = (
                SELECT ib.timezone_offset_minutes
                FROM import_src.input_buckets ib
                JOIN import_src.sources src_src ON src_src.id = ib.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.input_buckets.source_id
                  AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
                  AND ib.granularity_minutes = main.input_buckets.granularity_minutes
            ),
            left_clicks = main.input_buckets.left_clicks + (
                SELECT ib.left_clicks
                FROM import_src.input_buckets ib
                JOIN import_src.sources src_src ON src_src.id = ib.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.input_buckets.source_id
                  AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
                  AND ib.granularity_minutes = main.input_buckets.granularity_minutes
            ),
            right_clicks = main.input_buckets.right_clicks + (
                SELECT ib.right_clicks
                FROM import_src.input_buckets ib
                JOIN import_src.sources src_src ON src_src.id = ib.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.input_buckets.source_id
                  AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
                  AND ib.granularity_minutes = main.input_buckets.granularity_minutes
            ),
            middle_clicks = main.input_buckets.middle_clicks + (
                SELECT ib.middle_clicks
                FROM import_src.input_buckets ib
                JOIN import_src.sources src_src ON src_src.id = ib.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.input_buckets.source_id
                  AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
                  AND ib.granularity_minutes = main.input_buckets.granularity_minutes
            ),
            key_presses = main.input_buckets.key_presses + (
                SELECT ib.key_presses
                FROM import_src.input_buckets ib
                JOIN import_src.sources src_src ON src_src.id = ib.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.input_buckets.source_id
                  AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
                  AND ib.granularity_minutes = main.input_buckets.granularity_minutes
            ),
            mouse_distance_cm = main.input_buckets.mouse_distance_cm + (
                SELECT ib.mouse_distance_cm
                FROM import_src.input_buckets ib
                JOIN import_src.sources src_src ON src_src.id = ib.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.input_buckets.source_id
                  AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
                  AND ib.granularity_minutes = main.input_buckets.granularity_minutes
            ),
            scroll_vertical_cm = main.input_buckets.scroll_vertical_cm + (
                SELECT ib.scroll_vertical_cm
                FROM import_src.input_buckets ib
                JOIN import_src.sources src_src ON src_src.id = ib.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.input_buckets.source_id
                  AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
                  AND ib.granularity_minutes = main.input_buckets.granularity_minutes
            ),
            scroll_horizontal_cm = main.input_buckets.scroll_horizontal_cm + (
                SELECT ib.scroll_horizontal_cm
                FROM import_src.input_buckets ib
                JOIN import_src.sources src_src ON src_src.id = ib.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.input_buckets.source_id
                  AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
                  AND ib.granularity_minutes = main.input_buckets.granularity_minutes
            )
        WHERE EXISTS (
            SELECT 1
            FROM import_src.input_buckets ib
            JOIN import_src.sources src_src ON src_src.id = ib.source_id
            JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
            WHERE dest_src.id = main.input_buckets.source_id
              AND ib.bucket_start_utc = main.input_buckets.bucket_start_utc
              AND ib.granularity_minutes = main.input_buckets.granularity_minutes
        );

        INSERT INTO main.input_buckets (
            source_id,
            bucket_start_utc,
            bucket_end_utc,
            local_date,
            local_hour,
            timezone_offset_minutes,
            granularity_minutes,
            left_clicks,
            right_clicks,
            middle_clicks,
            key_presses,
            mouse_distance_cm,
            scroll_vertical_cm,
            scroll_horizontal_cm
        )
        SELECT
            dest_src.id,
            ib.bucket_start_utc,
            ib.bucket_end_utc,
            ib.local_date,
            ib.local_hour,
            ib.timezone_offset_minutes,
            ib.granularity_minutes,
            ib.left_clicks,
            ib.right_clicks,
            ib.middle_clicks,
            ib.key_presses,
            ib.mouse_distance_cm,
            ib.scroll_vertical_cm,
            ib.scroll_horizontal_cm
        FROM import_src.input_buckets ib
        JOIN import_src.sources src_src ON src_src.id = ib.source_id
        JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
        WHERE NOT EXISTS (
            SELECT 1
            FROM main.input_buckets dest
            WHERE dest.source_id = dest_src.id
              AND dest.bucket_start_utc = ib.bucket_start_utc
              AND dest.granularity_minutes = ib.granularity_minutes
        );

        UPDATE main.focus_buckets
        SET
            bucket_end_utc = (
                SELECT fb.bucket_end_utc
                FROM import_src.focus_buckets fb
                JOIN import_src.sources src_src ON src_src.id = fb.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.focus_buckets.source_id
                  AND fb.bucket_start_utc = main.focus_buckets.bucket_start_utc
                  AND fb.window_title = main.focus_buckets.window_title
                  AND fb.window_class = main.focus_buckets.window_class
            ),
            local_date = (
                SELECT fb.local_date
                FROM import_src.focus_buckets fb
                JOIN import_src.sources src_src ON src_src.id = fb.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.focus_buckets.source_id
                  AND fb.bucket_start_utc = main.focus_buckets.bucket_start_utc
                  AND fb.window_title = main.focus_buckets.window_title
                  AND fb.window_class = main.focus_buckets.window_class
            ),
            local_hour = (
                SELECT fb.local_hour
                FROM import_src.focus_buckets fb
                JOIN import_src.sources src_src ON src_src.id = fb.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.focus_buckets.source_id
                  AND fb.bucket_start_utc = main.focus_buckets.bucket_start_utc
                  AND fb.window_title = main.focus_buckets.window_title
                  AND fb.window_class = main.focus_buckets.window_class
            ),
            timezone_offset_minutes = (
                SELECT fb.timezone_offset_minutes
                FROM import_src.focus_buckets fb
                JOIN import_src.sources src_src ON src_src.id = fb.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.focus_buckets.source_id
                  AND fb.bucket_start_utc = main.focus_buckets.bucket_start_utc
                  AND fb.window_title = main.focus_buckets.window_title
                  AND fb.window_class = main.focus_buckets.window_class
            ),
            app_identifier = (
                SELECT fb.app_identifier
                FROM import_src.focus_buckets fb
                JOIN import_src.sources src_src ON src_src.id = fb.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.focus_buckets.source_id
                  AND fb.bucket_start_utc = main.focus_buckets.bucket_start_utc
                  AND fb.window_title = main.focus_buckets.window_title
                  AND fb.window_class = main.focus_buckets.window_class
            ),
            focus_seconds = main.focus_buckets.focus_seconds + (
                SELECT fb.focus_seconds
                FROM import_src.focus_buckets fb
                JOIN import_src.sources src_src ON src_src.id = fb.source_id
                JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
                WHERE dest_src.id = main.focus_buckets.source_id
                  AND fb.bucket_start_utc = main.focus_buckets.bucket_start_utc
                  AND fb.window_title = main.focus_buckets.window_title
                  AND fb.window_class = main.focus_buckets.window_class
            )
        WHERE EXISTS (
            SELECT 1
            FROM import_src.focus_buckets fb
            JOIN import_src.sources src_src ON src_src.id = fb.source_id
            JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
            WHERE dest_src.id = main.focus_buckets.source_id
              AND fb.bucket_start_utc = main.focus_buckets.bucket_start_utc
              AND fb.window_title = main.focus_buckets.window_title
              AND fb.window_class = main.focus_buckets.window_class
        );

        INSERT INTO main.focus_buckets (
            source_id,
            bucket_start_utc,
            bucket_end_utc,
            local_date,
            local_hour,
            timezone_offset_minutes,
            app_identifier,
            window_title,
            window_class,
            focus_seconds
        )
        SELECT
            dest_src.id,
            fb.bucket_start_utc,
            fb.bucket_end_utc,
            fb.local_date,
            fb.local_hour,
            fb.timezone_offset_minutes,
            fb.app_identifier,
            fb.window_title,
            fb.window_class,
            fb.focus_seconds
        FROM import_src.focus_buckets fb
        JOIN import_src.sources src_src ON src_src.id = fb.source_id
        JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
        WHERE NOT EXISTS (
            SELECT 1
            FROM main.focus_buckets dest
            WHERE dest.source_id = dest_src.id
              AND dest.bucket_start_utc = fb.bucket_start_utc
              AND dest.window_title = fb.window_title
              AND dest.window_class = fb.window_class
        );
        ",
    )?;

    tx.execute(
        "
        INSERT INTO imports (
            import_uuid,
            source_export_uuid,
            source_source_uuid,
            exported_at_utc,
            imported_at_utc,
            file_hash,
            schema_version,
            notes
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            Uuid::new_v4().to_string(),
            metadata.export_uuid,
            metadata.primary_source_uuid,
            metadata.exported_at_utc,
            Utc::now().to_rfc3339(),
            plan.source_file_hash,
            metadata.schema_version,
            notes,
        ],
    )?;
    tx.commit()?;

    detach_source(&destination)?;

    Ok(ImportResult {
        destination_backup_path: backup_path,
        plan,
    })
}

fn query_import_plan(
    conn: &Connection,
    metadata: &ExportMetadata,
    file_hash: &str,
    duplicate_reason: Option<String>,
) -> Result<ImportPlan> {
    let new_sources = scalar_query_u64(
        conn,
        "
        SELECT COUNT(*)
        FROM import_src.sources src
        LEFT JOIN main.sources dest ON dest.source_uuid = src.source_uuid
        WHERE dest.id IS NULL
        ",
    )?;

    let (
        new_input_buckets,
        updated_input_buckets,
        input_key_presses_delta,
        input_left_clicks_delta,
        input_right_clicks_delta,
        input_middle_clicks_delta,
        input_mouse_distance_cm_delta,
    ): (u64, u64, u64, u64, u64, u64, f64) = conn.query_row(
        "
            SELECT
                SUM(CASE WHEN existing.id IS NULL THEN 1 ELSE 0 END),
                SUM(CASE WHEN existing.id IS NOT NULL THEN 1 ELSE 0 END),
                COALESCE(SUM(ib.key_presses), 0),
                COALESCE(SUM(ib.left_clicks), 0),
                COALESCE(SUM(ib.right_clicks), 0),
                COALESCE(SUM(ib.middle_clicks), 0),
                COALESCE(SUM(ib.mouse_distance_cm), 0.0)
            FROM import_src.input_buckets ib
            JOIN import_src.sources src_src ON src_src.id = ib.source_id
            LEFT JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
            LEFT JOIN main.input_buckets existing
                ON existing.source_id = dest_src.id
               AND existing.bucket_start_utc = ib.bucket_start_utc
               AND existing.granularity_minutes = ib.granularity_minutes
            ",
        [],
        |row| {
            Ok((
                row.get::<_, Option<u64>>(0)?.unwrap_or(0),
                row.get::<_, Option<u64>>(1)?.unwrap_or(0),
                row.get::<_, Option<u64>>(2)?.unwrap_or(0),
                row.get::<_, Option<u64>>(3)?.unwrap_or(0),
                row.get::<_, Option<u64>>(4)?.unwrap_or(0),
                row.get::<_, Option<u64>>(5)?.unwrap_or(0),
                row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
            ))
        },
    )?;

    let (new_focus_buckets, updated_focus_buckets, focus_seconds_delta): (u64, u64, u64) = conn
        .query_row(
            "
            SELECT
                SUM(CASE WHEN existing.id IS NULL THEN 1 ELSE 0 END),
                SUM(CASE WHEN existing.id IS NOT NULL THEN 1 ELSE 0 END),
                COALESCE(SUM(fb.focus_seconds), 0)
            FROM import_src.focus_buckets fb
            JOIN import_src.sources src_src ON src_src.id = fb.source_id
            LEFT JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
            LEFT JOIN main.focus_buckets existing
                ON existing.source_id = dest_src.id
               AND existing.bucket_start_utc = fb.bucket_start_utc
               AND existing.window_title = fb.window_title
               AND existing.window_class = fb.window_class
            ",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<u64>>(0)?.unwrap_or(0),
                    row.get::<_, Option<u64>>(1)?.unwrap_or(0),
                    row.get::<_, Option<u64>>(2)?.unwrap_or(0),
                ))
            },
        )?;

    Ok(ImportPlan {
        source_export_uuid: metadata.export_uuid.clone(),
        source_file_hash: file_hash.to_string(),
        duplicate_import: duplicate_reason.is_some(),
        duplicate_reason,
        new_sources,
        new_input_buckets,
        updated_input_buckets,
        input_key_presses_delta,
        input_left_clicks_delta,
        input_right_clicks_delta,
        input_middle_clicks_delta,
        input_mouse_distance_cm_delta,
        new_focus_buckets,
        updated_focus_buckets,
        focus_seconds_delta,
    })
}

fn scalar_query_u64(conn: &Connection, sql: &str) -> Result<u64> {
    Ok(conn
        .query_row(sql, [], |row| row.get::<_, Option<u64>>(0))
        .with_context(|| "Scalar query failed")?
        .unwrap_or(0))
}

fn latest_export_metadata(conn: &Connection) -> Result<ExportMetadata> {
    conn.query_row(
        "
        SELECT export_uuid, primary_source_uuid, exported_at_utc, schema_version
        FROM exports
        ORDER BY id DESC
        LIMIT 1
        ",
        [],
        |row| {
            Ok(ExportMetadata {
                export_uuid: row.get(0)?,
                primary_source_uuid: row.get(1)?,
                exported_at_utc: row.get(2)?,
                schema_version: row.get(3)?,
            })
        },
    )
    .with_context(|| "Source snapshot does not contain export metadata; only snapshots created by --export-db can be imported")
}

fn latest_export_metadata_from_attached(conn: &Connection) -> Result<ExportMetadata> {
    conn.query_row(
        "
        SELECT export_uuid, primary_source_uuid, exported_at_utc, schema_version
        FROM import_src.exports
        ORDER BY id DESC
        LIMIT 1
        ",
        [],
        |row| {
            Ok(ExportMetadata {
                export_uuid: row.get(0)?,
                primary_source_uuid: row.get(1)?,
                exported_at_utc: row.get(2)?,
                schema_version: row.get(3)?,
            })
        },
    )
    .with_context(|| "Attached source snapshot does not contain export metadata")
}

fn existing_import_reason(
    conn: &Connection,
    export_uuid: &str,
    file_hash: &str,
) -> Result<Option<String>> {
    let existing_export_uuid: Option<String> = conn
        .query_row(
            "SELECT source_export_uuid FROM imports WHERE source_export_uuid = ?1",
            [export_uuid],
            |row| row.get(0),
        )
        .optional()?;
    if existing_export_uuid.is_some() {
        return Ok(Some(format!(
            "snapshot export UUID '{}' was already imported",
            export_uuid
        )));
    }

    let existing_hash: Option<String> = conn
        .query_row(
            "SELECT file_hash FROM imports WHERE file_hash = ?1",
            [file_hash],
            |row| row.get(0),
        )
        .optional()?;
    if existing_hash.is_some() {
        return Ok(Some(
            "snapshot file hash already exists in imports history".to_string(),
        ));
    }

    Ok(None)
}

fn validate_schema_version(conn: &Connection, label: &str) -> Result<i64> {
    let version: String = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .with_context(|| format!("Missing schema_version metadata in {label}"))?;
    let version = version
        .parse::<i64>()
        .with_context(|| format!("Invalid schema_version value in {label}: {version}"))?;

    if version != SCHEMA_VERSION {
        bail!(
            "{label} uses schema version {version}, but life-monitor expects {}",
            SCHEMA_VERSION
        );
    }
    Ok(version)
}

fn run_integrity_check(conn: &Connection, label: &str) -> Result<()> {
    let status: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .with_context(|| format!("Failed to run integrity_check on {label}"))?;
    if status != "ok" {
        bail!("{label} failed integrity_check: {status}");
    }
    Ok(())
}

fn attach_source(conn: &Connection, source_snapshot_path: &Path) -> Result<()> {
    conn.execute(
        "ATTACH DATABASE ?1 AS import_src",
        [source_snapshot_path.to_string_lossy().to_string()],
    )
    .with_context(|| {
        format!(
            "Failed to ATTACH source snapshot at {}",
            source_snapshot_path.display()
        )
    })?;
    Ok(())
}

fn detach_source(conn: &Connection) -> Result<()> {
    conn.execute_batch("DETACH DATABASE import_src")
        .with_context(|| "Failed to DETACH imported source database")
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("Failed to open file for hashing: {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes = file.read(&mut buffer)?;
        if bytes == 0 {
            break;
        }
        hasher.update(&buffer[..bytes]);
    }

    let digest = hasher.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn backup_database_contents(source: &Connection, destination_path: &Path) -> Result<()> {
    let mut snapshot = Connection::open(destination_path).with_context(|| {
        format!(
            "Failed to create sqlite backup destination: {}",
            destination_path.display()
        )
    })?;
    let backup = backup::Backup::new(source, &mut snapshot)?;
    backup.run_to_completion(32, Duration::from_millis(50), None)?;
    Ok(())
}

fn default_pre_import_backup_path(destination_db_path: &Path) -> PathBuf {
    let timestamp = Utc::now().format("%Y%m%d%H%M%S");
    let stem = destination_db_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("data");
    let filename = format!("{stem}.pre-import-{timestamp}.sqlite");
    destination_db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(filename)
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::sync::{Mutex, OnceLock};

    fn unique_temp_db(name: &str) -> PathBuf {
        let suffix = Uuid::new_v4();
        std::env::temp_dir().join(format!("life-monitor-{name}-{suffix}.db"))
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn build_test_db(path: &Path) -> Result<Connection> {
        let conn = open_con_at(path)?;
        setup_database(&conn)?;
        Ok(conn)
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

    #[test]
    fn db_config_remembers_and_overwrites_last_db_path() -> Result<()> {
        let _guard = env_lock().lock().unwrap();
        let data_dir = unique_temp_db("remembered-config-dir");
        std::fs::create_dir_all(&data_dir)?;
        std::env::set_var("LIFE_MONITOR_DATA_DIR", &data_dir);

        let first_path = unique_temp_db("remembered-first");
        let second_path = unique_temp_db("remembered-second");

        let first = DbConfig::from_cli_path(Some(first_path.clone()))?;
        assert_eq!(first.db_path, first_path);
        assert_eq!(first.source, DbPathSource::Cli);

        let remembered = DbConfig::from_cli_path(None)?;
        assert_eq!(remembered.db_path, first_path);
        assert_eq!(remembered.source, DbPathSource::Remembered);

        let second = DbConfig::from_cli_path(Some(second_path.clone()))?;
        assert_eq!(second.db_path, second_path);

        let remembered_again = DbConfig::from_cli_path(None)?;
        assert_eq!(remembered_again.db_path, second_path);
        assert_eq!(remembered_again.source, DbPathSource::Remembered);

        std::env::remove_var("LIFE_MONITOR_DATA_DIR");
        fs::remove_dir_all(data_dir)?;
        Ok(())
    }

    fn sample_second_input_row() -> InputBucketRecord {
        InputBucketRecord {
            bucket_start_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 15, 0).unwrap(),
            bucket_end_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 30, 0).unwrap(),
            local_hour: 9,
            key_presses: 7,
            left_clicks: 3,
            mouse_distance_cm: 1.5,
            ..sample_input_row()
        }
    }

    fn sample_second_focus_row() -> FocusBucketRecord {
        FocusBucketRecord {
            bucket_start_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 15, 0).unwrap(),
            bucket_end_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 30, 0).unwrap(),
            local_hour: 9,
            window_title: "Mail".to_string(),
            focus_seconds: 45,
            ..sample_focus_row()
        }
    }

    #[test]
    fn resolve_db_path_uses_custom_location() -> Result<()> {
        let path = unique_temp_db("custom-path").join("nested/data.db");
        let resolved = resolve_db_path(Some(&path))?;
        assert_eq!(resolved, path);
        assert!(resolved.parent().unwrap().exists());
        Ok(())
    }

    #[test]
    fn resolve_db_path_uses_data_db_inside_existing_directory() -> Result<()> {
        let dir = unique_temp_db("custom-dir-existing");
        fs::create_dir_all(&dir)?;

        let resolved = resolve_db_path(Some(&dir))?;

        assert_eq!(resolved, dir.join("data.db"));
        assert!(resolved.parent().unwrap().exists());
        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn resolve_db_path_creates_missing_directory_and_uses_data_db() -> Result<()> {
        let dir = std::env::temp_dir().join(format!(
            "life-monitor-custom-dir-missing-{}",
            Uuid::new_v4()
        ));

        let resolved = resolve_db_path(Some(&dir))?;

        assert_eq!(resolved, dir.join("data.db"));
        assert!(dir.exists());
        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn setup_database_creates_metadata_tables() -> Result<()> {
        let path = unique_temp_db("schema");
        let conn = build_test_db(&path)?;

        let schema_version: String = conn.query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(schema_version, SCHEMA_VERSION.to_string());

        let source = get_source(&conn, DEFAULT_SOURCE_ID)?;
        assert_eq!(source.id, DEFAULT_SOURCE_ID);
        assert!(!source.source_uuid.is_empty());
        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn export_database_creates_snapshot_with_export_metadata() -> Result<()> {
        let source_path = unique_temp_db("export-source");
        let export_path = unique_temp_db("export-snapshot");
        let conn = build_test_db(&source_path)?;
        insert_input_buckets(&conn, &[sample_input_row()])?;

        let export = export_database(&source_path, &export_path)?;
        let snapshot = open_con_at(&export_path)?;
        let metadata = latest_export_metadata(&snapshot)?;

        assert_eq!(metadata.export_uuid, export.export_uuid);
        assert_eq!(
            scalar_query_u64(&snapshot, "SELECT COUNT(*) FROM input_buckets")?,
            1
        );

        drop(snapshot);
        drop(conn);
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        Ok(())
    }

    #[test]
    fn plan_import_marks_duplicate_snapshot() -> Result<()> {
        let destination_path = unique_temp_db("import-dest");
        let source_path = unique_temp_db("import-source");
        let export_path = unique_temp_db("import-export");

        let destination = build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        insert_input_buckets(&source, &[sample_input_row()])?;

        let export = export_database(&source_path, &export_path)?;
        destination.execute(
            "
            INSERT INTO imports (
                import_uuid,
                source_export_uuid,
                source_source_uuid,
                exported_at_utc,
                imported_at_utc,
                file_hash,
                schema_version,
                notes
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
            ",
            params![
                Uuid::new_v4().to_string(),
                export.export_uuid,
                "source-uuid",
                Utc::now().to_rfc3339(),
                Utc::now().to_rfc3339(),
                file_sha256(&export_path)?,
                SCHEMA_VERSION,
            ],
        )?;

        let plan = plan_import(&destination_path, &export_path)?;
        assert!(plan.duplicate_import);

        drop(source);
        drop(destination);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        Ok(())
    }

    #[test]
    fn import_snapshot_merges_rows_and_records_history() -> Result<()> {
        let destination_path = unique_temp_db("merge-dest");
        let source_path = unique_temp_db("merge-source");
        let export_path = unique_temp_db("merge-export");

        let destination = build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        let destination_source_uuid: String = destination.query_row(
            "SELECT source_uuid FROM sources WHERE id = ?1",
            [DEFAULT_SOURCE_ID],
            |row| row.get(0),
        )?;
        source.execute(
            "UPDATE sources SET source_uuid = ?1 WHERE id = ?2",
            params![destination_source_uuid, DEFAULT_SOURCE_ID],
        )?;
        insert_input_buckets(&destination, &[sample_input_row()])?;
        insert_input_buckets(
            &source,
            &[InputBucketRecord {
                key_presses: 9,
                left_clicks: 4,
                ..sample_input_row()
            }],
        )?;
        insert_focus_buckets(&source, &[sample_focus_row()])?;

        export_database(&source_path, &export_path)?;
        let result = import_snapshot(&destination_path, &export_path, Some("sync test"))?;

        let merged = open_con_at(&destination_path)?;
        let stored = merged.query_row(
            "SELECT left_clicks, key_presses FROM input_buckets",
            [],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
        )?;
        let imports_count = scalar_query_u64(&merged, "SELECT COUNT(*) FROM imports")?;
        let focus_count = scalar_query_u64(&merged, "SELECT COUNT(*) FROM focus_buckets")?;

        assert_eq!(stored, (6, 14));
        assert_eq!(imports_count, 1);
        assert_eq!(focus_count, 1);
        assert!(result.destination_backup_path.exists());

        drop(merged);
        drop(source);
        drop(destination);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        fs::remove_file(result.destination_backup_path)?;
        Ok(())
    }

    #[test]
    fn import_snapshot_with_different_source_uuid_keeps_rows_separate() -> Result<()> {
        let destination_path = unique_temp_db("multi-source-dest");
        let source_path = unique_temp_db("multi-source-source");
        let export_path = unique_temp_db("multi-source-export");

        let destination = build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        insert_input_buckets(&destination, &[sample_input_row()])?;
        insert_input_buckets(&source, &[sample_input_row()])?;

        export_database(&source_path, &export_path)?;
        import_snapshot(&destination_path, &export_path, Some("new source"))?;

        let merged = open_con_at(&destination_path)?;
        let source_count = scalar_query_u64(&merged, "SELECT COUNT(*) FROM sources")?;
        let input_count = scalar_query_u64(&merged, "SELECT COUNT(*) FROM input_buckets")?;

        assert_eq!(source_count, 2);
        assert_eq!(input_count, 2);

        drop(merged);
        drop(source);
        drop(destination);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        Ok(())
    }

    #[test]
    fn plan_import_rejects_corrupted_snapshot_file() -> Result<()> {
        let destination_path = unique_temp_db("corrupt-dest");
        let source_path = unique_temp_db("corrupt-export");
        build_test_db(&destination_path)?;
        fs::write(&source_path, b"not-a-sqlite-database")?;

        let err = plan_import(&destination_path, &source_path).unwrap_err();
        assert!(
            err.to_string().contains("Failed to open source snapshot")
                || err.to_string().contains("integrity")
        );

        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        Ok(())
    }

    #[test]
    fn plan_import_rejects_schema_version_mismatch() -> Result<()> {
        let destination_path = unique_temp_db("schema-mismatch-dest");
        let source_path = unique_temp_db("schema-mismatch-source");
        let export_path = unique_temp_db("schema-mismatch-export");

        build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        insert_input_buckets(&source, &[sample_input_row()])?;
        export_database(&source_path, &export_path)?;

        let export_conn = open_con_at(&export_path)?;
        export_conn.execute(
            "UPDATE schema_meta SET value = '999' WHERE key = 'schema_version'",
            [],
        )?;

        let err = plan_import(&destination_path, &export_path).unwrap_err();
        assert!(err.to_string().contains("schema version"));

        drop(export_conn);
        drop(source);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        Ok(())
    }

    #[test]
    fn import_snapshot_handles_partial_bucket_and_window_overlaps() -> Result<()> {
        let destination_path = unique_temp_db("partial-overlap-dest");
        let source_path = unique_temp_db("partial-overlap-source");
        let export_path = unique_temp_db("partial-overlap-export");

        let destination = build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        let destination_source_uuid: String = destination.query_row(
            "SELECT source_uuid FROM sources WHERE id = ?1",
            [DEFAULT_SOURCE_ID],
            |row| row.get(0),
        )?;
        source.execute(
            "UPDATE sources SET source_uuid = ?1 WHERE id = ?2",
            params![destination_source_uuid, DEFAULT_SOURCE_ID],
        )?;

        insert_input_buckets(&destination, &[sample_input_row()])?;
        insert_focus_buckets(&destination, &[sample_focus_row()])?;

        insert_input_buckets(&source, &[sample_input_row(), sample_second_input_row()])?;
        insert_focus_buckets(&source, &[sample_focus_row(), sample_second_focus_row()])?;

        export_database(&source_path, &export_path)?;
        import_snapshot(&destination_path, &export_path, None)?;

        let merged = open_con_at(&destination_path)?;
        let input_rows = scalar_query_u64(&merged, "SELECT COUNT(*) FROM input_buckets")?;
        let focus_rows = scalar_query_u64(&merged, "SELECT COUNT(*) FROM focus_buckets")?;
        let overlapping = merged.query_row(
            "SELECT key_presses FROM input_buckets WHERE bucket_start_utc = ?1",
            [sample_input_row().bucket_start_utc.to_rfc3339()],
            |row| row.get::<_, u64>(0),
        )?;

        assert_eq!(input_rows, 2);
        assert_eq!(focus_rows, 2);
        assert_eq!(overlapping, 10);

        drop(merged);
        drop(source);
        drop(destination);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        Ok(())
    }

    #[test]
    fn duplicate_detection_prefers_export_uuid_even_if_snapshot_hash_changes() -> Result<()> {
        let destination_path = unique_temp_db("duplicate-export-uuid-dest");
        let source_path = unique_temp_db("duplicate-export-uuid-source");
        let export_path = unique_temp_db("duplicate-export-uuid-export");

        let destination = build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        insert_input_buckets(&source, &[sample_input_row()])?;
        export_database(&source_path, &export_path)?;
        import_snapshot(&destination_path, &export_path, Some("first import"))?;

        let export_conn = open_con_at(&export_path)?;
        export_conn.execute(
            "UPDATE exports SET notes = 'mutated after import' WHERE id = (SELECT MAX(id) FROM exports)",
            [],
        )?;

        let plan = plan_import(&destination_path, &export_path)?;
        assert!(plan.duplicate_import);
        assert!(plan
            .duplicate_reason
            .unwrap_or_default()
            .contains("export UUID"));

        drop(export_conn);
        drop(source);
        drop(destination);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        let backup_dir = std::env::temp_dir();
        for entry in fs::read_dir(backup_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.contains("duplicate-export-uuid-dest") && name.contains("pre-import") {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }
}
