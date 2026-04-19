use anyhow::{Context, Result};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::info;

use crate::common::program_data_dir;

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
