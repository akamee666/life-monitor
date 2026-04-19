use anyhow::{bail, Context, Result};
use chrono::Utc;
use rusqlite::backup;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::rows::open_con_at;
use super::schema::SCHEMA_VERSION;

pub(crate) fn scalar_query_u64(conn: &Connection, sql: &str) -> Result<u64> {
    Ok(conn
        .query_row(sql, [], |row| row.get::<_, Option<u64>>(0))
        .with_context(|| "Scalar query failed")?
        .unwrap_or(0))
}

pub(crate) fn validate_schema_version(conn: &Connection, label: &str) -> Result<i64> {
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

pub(crate) fn run_integrity_check(conn: &Connection, label: &str) -> Result<()> {
    let status: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .with_context(|| format!("Failed to run integrity_check on {label}"))?;
    if status != "ok" {
        bail!("{label} failed integrity_check: {status}");
    }
    Ok(())
}

pub(crate) fn attach_source(conn: &Connection, source_snapshot_path: &Path) -> Result<()> {
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

pub(crate) fn detach_source(conn: &Connection) -> Result<()> {
    conn.execute_batch("DETACH DATABASE import_src")
        .with_context(|| "Failed to DETACH imported source database")
}

pub(crate) fn file_sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("Failed to open '{}' for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];

    loop {
        let bytes = file
            .read(&mut buf)
            .with_context(|| format!("Failed while hashing '{}'", path.display()))?;
        if bytes == 0 {
            break;
        }
        hasher.update(&buf[..bytes]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn backup_database_contents(source: &Connection, backup_path: &Path) -> Result<()> {
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for SQLite backup: {}",
                parent.display()
            )
        })?;
    }

    let mut destination = open_con_at(backup_path)?;
    let backup = backup::Backup::new(source, &mut destination)
        .with_context(|| "Failed to initialize SQLite backup operation")?;
    backup
        .run_to_completion(32, Duration::from_millis(50), None)
        .with_context(|| "Failed to complete SQLite backup operation")?;
    Ok(())
}

pub(crate) fn default_pre_import_backup_path(destination_db_path: &Path) -> PathBuf {
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let stem = destination_db_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("data");
    let file_name = format!("{stem}.pre-import-{timestamp}.sqlite");

    destination_db_path
        .parent()
        .map(|parent| parent.join(&file_name))
        .unwrap_or_else(|| PathBuf::from(file_name))
}
