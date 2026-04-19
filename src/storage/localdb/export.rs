use anyhow::{bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;
use uuid::Uuid;

use crate::common::DEFAULT_SOURCE_ID;
use crate::utils::lock::acquire_db_operation_lock;

use super::integrity::{backup_database_contents, run_integrity_check};
use super::rows::{get_source, open_con_at};
use super::schema::{setup_database, SCHEMA_VERSION};

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

pub(crate) fn latest_export_metadata(conn: &Connection) -> Result<ExportMetadata> {
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

pub(crate) fn latest_export_metadata_from_attached(conn: &Connection) -> Result<ExportMetadata> {
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
