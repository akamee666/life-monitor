//! Storage backend abstraction.
use crate::common::*;
use crate::storage::localdb::*;
#[cfg(feature = "multi-sync")]
use crate::sync::{apply_local_focus_rows, apply_local_input_rows, apply_local_source};
use crate::utils::lock::acquire_db_operation_lock;

use rusqlite::Connection;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::*;

use tracing::*;

#[allow(async_fn_in_trait)]
pub trait DataStore {
    async fn store_keys_data(&self, rows: &[InputBucketRecord]) -> Result<()>;
    async fn store_proc_data(&self, rows: &[FocusBucketRecord]) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct LocalDb {
    con: Arc<Mutex<Connection>>,
    source_id: i64,
    db_path: PathBuf,
    _session: Arc<SessionLifecycle>,
}

#[derive(Debug)]
struct SessionLifecycle {
    con: Arc<Mutex<Connection>>,
    session_uuid: String,
}

impl LocalDb {
    pub fn new(config: DbConfig, should_clear: bool) -> Result<Self> {
        if should_clear {
            info!("Clean argument provided, cleaning database!");
            clear_database(&config.db_path).context("Failed to clear database")?;
        };

        let conn = open_con_at(&config.db_path).with_context(|| {
            if config.source == DbPathSource::Remembered {
                format_remembered_db_open_error(&config.db_path)
            } else {
                format!(
                    "Failed to open connection with sqlite database at '{}'",
                    config.db_path.display()
                )
            }
        })?;
        setup_database(&conn).context("Failed to properly setup sqlite database")?;
        let session_uuid = begin_session(&conn, DEFAULT_SOURCE_ID, std::env::consts::OS)
            .with_context(|| "Failed to record the startup of the current collection session")?;
        #[cfg(feature = "multi-sync")]
        {
            let source = get_source(&conn, DEFAULT_SOURCE_ID)?;
            apply_local_source(&conn, &source)
                .with_context(|| "Failed to seed the local source row into the sync outbox")?;
        }

        info!(
            "Backend using SQLite successfully initialized at {}.",
            config.db_path.display()
        );
        let shared_con = Arc::new(Mutex::new(conn));
        Ok(Self {
            con: shared_con.clone(),
            source_id: DEFAULT_SOURCE_ID,
            db_path: config.db_path,
            _session: Arc::new(SessionLifecycle {
                con: shared_con,
                session_uuid,
            }),
        })
    }

    pub fn source_id(&self) -> i64 {
        self.source_id
    }

    pub fn bucket_granularity_minutes(&self) -> u32 {
        DEFAULT_BUCKET_MINUTES as u32
    }

    #[cfg(feature = "multi-sync")]
    pub fn shared_connection(&self) -> Arc<Mutex<Connection>> {
        self.con.clone()
    }

    #[cfg(feature = "multi-sync")]
    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }
}

fn format_remembered_db_open_error(path: &std::path::Path) -> String {
    format!(
        "Failed to open the remembered database path '{}'.\nThis usually means the path is no longer available, such as an unmounted or disconnected network share.\nWhat you can do:\n- mount or reconnect the share again so Vigil can access it\n- run Vigil with --db-path <NEW_PATH> to switch to another database now\n- later, import the old database or a snapshot once it is available again",
        path.display()
    )
}

impl DataStore for LocalDb {
    async fn store_keys_data(&self, rows: &[InputBucketRecord]) -> Result<()> {
        let rows = rows.to_vec();
        let con = self.con.clone();
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            let _op_lock = acquire_db_operation_lock(&db_path)?;
            let mut con = con
                .lock()
                .map_err(|_| anyhow!("database connection lock was poisoned"))?;
            let tx = con.transaction()?;
            #[cfg(feature = "multi-sync")]
            apply_local_input_rows(&tx, &rows)
                .context("Failed to insert input bucket rows into sqlite database")?;
            #[cfg(not(feature = "multi-sync"))]
            insert_input_buckets(&tx, &rows)
                .context("Failed to insert input bucket rows into sqlite database")?;
            tx.commit().context("Failed to commit input bucket rows")
        })
        .await?
    }

    async fn store_proc_data(&self, rows: &[FocusBucketRecord]) -> Result<()> {
        let rows = rows.to_vec();
        let con = self.con.clone();
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            let _op_lock = acquire_db_operation_lock(&db_path)?;
            let mut con = con
                .lock()
                .map_err(|_| anyhow!("database connection lock was poisoned"))?;
            let tx = con.transaction()?;
            #[cfg(feature = "multi-sync")]
            apply_local_focus_rows(&tx, &rows)
                .context("Failed to insert focus bucket rows into sqlite database")?;
            #[cfg(not(feature = "multi-sync"))]
            insert_focus_buckets(&tx, &rows)
                .context("Failed to insert focus bucket rows into sqlite database")?;
            tx.commit().context("Failed to commit focus bucket rows")
        })
        .await?
    }
}

impl Drop for SessionLifecycle {
    fn drop(&mut self) {
        if let Some(conn) = self.con.lock().ok() {
            if let Err(err) = end_session(&conn, &self.session_uuid) {
                error!(
                    "Failed to finalize collection session {}: {err:#}",
                    self.session_uuid
                );
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum StorageBackend {
    Local(LocalDb),
}

impl StorageBackend {
    pub fn source_id(&self) -> i64 {
        match self {
            StorageBackend::Local(db) => db.source_id(),
        }
    }

    pub fn bucket_granularity_minutes(&self) -> u32 {
        match self {
            StorageBackend::Local(db) => db.bucket_granularity_minutes(),
        }
    }
}

impl DataStore for StorageBackend {
    async fn store_keys_data(&self, rows: &[InputBucketRecord]) -> Result<()> {
        match self {
            StorageBackend::Local(db) => db.store_keys_data(rows).await,
        }
    }

    async fn store_proc_data(&self, rows: &[FocusBucketRecord]) -> Result<()> {
        match self {
            StorageBackend::Local(db) => db.store_proc_data(rows).await,
        }
    }
}
