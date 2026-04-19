use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use chrono::Utc;

use super::types::{
    ChangePayload, EntityType, FocusBucketChange, InputBucketChange, PullResponse, PushAck,
    PushBatch, RemoteChange, RemoteStatus, SourceChange,
};

#[allow(async_fn_in_trait)]
pub trait SyncRemote: Send + Sync {
    async fn push_batch(&self, batch: PushBatch) -> Result<PushAck>;
    async fn pull_since(
        &self,
        own_source_uuid: &str,
        last_pulled_revision: i64,
    ) -> Result<PullResponse>;
    async fn status(&self) -> Result<RemoteStatus>;
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Default)]
pub struct InMemoryRemote {
    inner: Arc<Mutex<RemoteState>>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Default)]
struct RemoteState {
    head_revision: i64,
    applied_batches: HashMap<String, (String, i64)>,
    source_rows: BTreeMap<String, SourceChange>,
    input_rows: BTreeMap<String, InputBucketChange>,
    focus_rows: BTreeMap<String, FocusBucketChange>,
    change_log: Vec<RemoteChange>,
}

impl InMemoryRemote {
    #[cfg_attr(not(test), allow(dead_code))]
    fn append_change(state: &mut RemoteState, payload: ChangePayload) {
        let revision = state.head_revision + 1;
        state.head_revision = revision;

        match &payload {
            ChangePayload::Source(change) => {
                state
                    .source_rows
                    .insert(change.entity_key(), change.clone());
            }
            ChangePayload::InputBucket(change) => {
                state.input_rows.insert(change.entity_key(), change.clone());
            }
            ChangePayload::FocusBucket(change) => {
                state.focus_rows.insert(change.entity_key(), change.clone());
            }
        }

        state.change_log.push(RemoteChange { revision, payload });
    }
}

impl SyncRemote for InMemoryRemote {
    async fn push_batch(&self, batch: PushBatch) -> Result<PushAck> {
        let mut state = self.inner.lock().unwrap();
        if let Some((source_uuid, applied_revision)) = state.applied_batches.get(&batch.batch_uuid)
        {
            if source_uuid != &batch.source_uuid {
                bail!(
                    "batch {} was previously applied for another source",
                    batch.batch_uuid
                );
            }
            return Ok(PushAck {
                applied_revision: *applied_revision,
                remote_head_revision: state.head_revision,
            });
        }

        let mut applied_revision = state.head_revision;
        for change in batch.source_changes {
            if change.source_uuid != batch.source_uuid {
                bail!("push batch contains foreign-source payload");
            }
            Self::append_change(&mut state, ChangePayload::Source(change));
            applied_revision = state.head_revision;
        }
        for change in batch.input_changes {
            if change.source_uuid != batch.source_uuid {
                bail!("push batch contains foreign-source payload");
            }
            Self::append_change(&mut state, ChangePayload::InputBucket(change));
            applied_revision = state.head_revision;
        }
        for change in batch.focus_changes {
            if change.source_uuid != batch.source_uuid {
                bail!("push batch contains foreign-source payload");
            }
            Self::append_change(&mut state, ChangePayload::FocusBucket(change));
            applied_revision = state.head_revision;
        }

        state
            .applied_batches
            .insert(batch.batch_uuid, (batch.source_uuid, applied_revision));

        Ok(PushAck {
            applied_revision,
            remote_head_revision: state.head_revision,
        })
    }

    async fn pull_since(
        &self,
        own_source_uuid: &str,
        last_pulled_revision: i64,
    ) -> Result<PullResponse> {
        let state = self.inner.lock().unwrap();
        let changes = state
            .change_log
            .iter()
            .filter(|change| change.revision > last_pulled_revision)
            .filter(|change| {
                !(last_pulled_revision == 0 && change.payload.source_uuid() == own_source_uuid)
            })
            .cloned()
            .collect::<Vec<_>>();
        Ok(PullResponse {
            remote_head_revision: state.head_revision,
            changes,
        })
    }

    async fn status(&self) -> Result<RemoteStatus> {
        let state = self.inner.lock().unwrap();
        Ok(RemoteStatus {
            remote_head_revision: state.head_revision,
        })
    }
}

#[cfg(feature = "multi-sync")]
#[derive(Debug, Clone)]
pub struct SqldRemote {
    database: Arc<libsql::Database>,
}

#[cfg(feature = "multi-sync")]
impl SqldRemote {
    pub async fn new(remote_url: &str, auth_token: &str) -> Result<Self> {
        let builder = libsql::Builder::new_remote(remote_url.to_string(), auth_token.to_string());
        let database = builder.build().await.with_context(|| {
            format!("Failed to initialize remote sqld connection for {remote_url}")
        })?;
        let remote = Self {
            database: Arc::new(database),
        };
        remote.ensure_remote_schema().await?;
        Ok(remote)
    }

    async fn ensure_remote_schema(&self) -> Result<()> {
        let conn = self
            .database
            .as_ref()
            .connect()
            .with_context(|| "Failed to open remote sqld connection")?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sources (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_uuid TEXT NOT NULL UNIQUE,
                source_name TEXT NOT NULL,
                platform TEXT NOT NULL,
                created_at_utc TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS input_buckets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
                UNIQUE(source_uuid, bucket_start_utc, granularity_minutes)
            );

            CREATE TABLE IF NOT EXISTS focus_buckets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
                UNIQUE(source_uuid, bucket_start_utc, window_title, window_class)
            );

            CREATE TABLE IF NOT EXISTS sync_applied_batches (
                batch_uuid TEXT PRIMARY KEY,
                source_uuid TEXT NOT NULL,
                applied_revision INTEGER NOT NULL,
                applied_at_utc TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sync_revisions (
                revision INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at_utc TEXT NOT NULL,
                source_uuid TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                entity_key TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sync_source_changes (
                revision INTEGER PRIMARY KEY,
                source_uuid TEXT NOT NULL,
                source_name TEXT NOT NULL,
                platform TEXT NOT NULL,
                created_at_utc TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sync_input_changes (
                revision INTEGER PRIMARY KEY,
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
                scroll_horizontal_cm REAL NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sync_focus_changes (
                revision INTEGER PRIMARY KEY,
                source_uuid TEXT NOT NULL,
                bucket_start_utc TEXT NOT NULL,
                bucket_end_utc TEXT NOT NULL,
                local_date TEXT NOT NULL,
                local_hour INTEGER NOT NULL,
                timezone_offset_minutes INTEGER NOT NULL,
                app_identifier TEXT NOT NULL,
                window_title TEXT NOT NULL,
                window_class TEXT NOT NULL,
                focus_seconds INTEGER NOT NULL
            );
            ",
        )
        .await
        .with_context(|| "Failed to initialize remote sync schema")?;
        Ok(())
    }

    async fn remote_head_revision(&self) -> Result<i64> {
        let conn = self.database.as_ref().connect()?;
        let mut rows = conn
            .query("SELECT COALESCE(MAX(revision), 0) FROM sync_revisions", ())
            .await?;
        let row = rows.next().await?.context("missing revision row")?;
        Ok(*row.get_value(0)?.as_integer().unwrap_or(&0))
    }

    async fn insert_revision(
        tx: &libsql::Transaction,
        source_uuid: &str,
        entity_type: EntityType,
        entity_key: &str,
    ) -> Result<i64> {
        tx.execute(
            "
            INSERT INTO sync_revisions (created_at_utc, source_uuid, entity_type, entity_key)
            VALUES (?1, ?2, ?3, ?4)
            ",
            libsql::params![
                Utc::now().to_rfc3339(),
                source_uuid.to_string(),
                entity_type.as_str(),
                entity_key.to_string()
            ],
        )
        .await?;
        Ok(tx.last_insert_rowid())
    }

    async fn load_change_by_revision(
        conn: &libsql::Connection,
        revision: i64,
        entity_type: &str,
    ) -> Result<ChangePayload> {
        match entity_type {
            "source" => Self::load_source_change(conn, revision)
                .await
                .map(ChangePayload::Source),
            "input_bucket" => Self::load_input_change(conn, revision)
                .await
                .map(ChangePayload::InputBucket),
            "focus_bucket" => Self::load_focus_change(conn, revision)
                .await
                .map(ChangePayload::FocusBucket),
            other => bail!("unsupported sync entity type {other}"),
        }
    }

    async fn load_source_change(conn: &libsql::Connection, revision: i64) -> Result<SourceChange> {
        let mut rows = conn
            .query(
                "
                SELECT source_uuid, source_name, platform, created_at_utc
                FROM sync_source_changes
                WHERE revision = ?1
                ",
                libsql::params![revision],
            )
            .await?;
        let row = rows.next().await?.context("missing source change row")?;
        Ok(SourceChange {
            source_uuid: row
                .get_value(0)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            source_name: row
                .get_value(1)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            platform: row
                .get_value(2)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            created_at_utc: row
                .get_value(3)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
        })
    }

    async fn load_input_change(
        conn: &libsql::Connection,
        revision: i64,
    ) -> Result<InputBucketChange> {
        let mut rows = conn
            .query(
                "
                SELECT source_uuid, bucket_start_utc, bucket_end_utc, local_date, local_hour,
                       timezone_offset_minutes, granularity_minutes, left_clicks, right_clicks,
                       middle_clicks, key_presses, mouse_distance_cm, scroll_vertical_cm,
                       scroll_horizontal_cm
                FROM sync_input_changes
                WHERE revision = ?1
                ",
                libsql::params![revision],
            )
            .await?;
        let row = rows.next().await?.context("missing input change row")?;
        Ok(InputBucketChange {
            source_uuid: row
                .get_value(0)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            bucket_start_utc: row
                .get_value(1)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            bucket_end_utc: row
                .get_value(2)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            local_date: row
                .get_value(3)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            local_hour: *row.get_value(4)?.as_integer().unwrap_or(&0) as u32,
            timezone_offset_minutes: *row.get_value(5)?.as_integer().unwrap_or(&0) as i32,
            granularity_minutes: *row.get_value(6)?.as_integer().unwrap_or(&0) as u32,
            left_clicks: *row.get_value(7)?.as_integer().unwrap_or(&0) as u64,
            right_clicks: *row.get_value(8)?.as_integer().unwrap_or(&0) as u64,
            middle_clicks: *row.get_value(9)?.as_integer().unwrap_or(&0) as u64,
            key_presses: *row.get_value(10)?.as_integer().unwrap_or(&0) as u64,
            mouse_distance_cm: *row.get_value(11)?.as_real().unwrap_or(&0.0),
            scroll_vertical_cm: *row.get_value(12)?.as_real().unwrap_or(&0.0),
            scroll_horizontal_cm: *row.get_value(13)?.as_real().unwrap_or(&0.0),
        })
    }

    async fn load_focus_change(
        conn: &libsql::Connection,
        revision: i64,
    ) -> Result<FocusBucketChange> {
        let mut rows = conn
            .query(
                "
                SELECT source_uuid, bucket_start_utc, bucket_end_utc, local_date, local_hour,
                       timezone_offset_minutes, app_identifier, window_title, window_class,
                       focus_seconds
                FROM sync_focus_changes
                WHERE revision = ?1
                ",
                libsql::params![revision],
            )
            .await?;
        let row = rows.next().await?.context("missing focus change row")?;
        Ok(FocusBucketChange {
            source_uuid: row
                .get_value(0)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            bucket_start_utc: row
                .get_value(1)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            bucket_end_utc: row
                .get_value(2)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            local_date: row
                .get_value(3)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            local_hour: *row.get_value(4)?.as_integer().unwrap_or(&0) as u32,
            timezone_offset_minutes: *row.get_value(5)?.as_integer().unwrap_or(&0) as i32,
            app_identifier: row
                .get_value(6)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            window_title: row
                .get_value(7)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            window_class: row
                .get_value(8)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default(),
            focus_seconds: *row.get_value(9)?.as_integer().unwrap_or(&0) as u64,
        })
    }
}

#[cfg(feature = "multi-sync")]
impl SyncRemote for SqldRemote {
    async fn push_batch(&self, batch: PushBatch) -> Result<PushAck> {
        let conn = self.database.as_ref().connect()?;
        let mut existing = conn
            .query(
                "SELECT applied_revision FROM sync_applied_batches WHERE batch_uuid = ?1",
                libsql::params![batch.batch_uuid.clone()],
            )
            .await?;
        if let Some(row) = existing.next().await? {
            return Ok(PushAck {
                applied_revision: *row.get_value(0)?.as_integer().unwrap_or(&0),
                remote_head_revision: self.remote_head_revision().await?,
            });
        }

        let tx = conn.transaction().await?;
        let mut applied_revision = self.remote_head_revision().await?;

        for change in &batch.source_changes {
            if change.source_uuid != batch.source_uuid {
                bail!("push batch contains foreign-source payload");
            }
            tx.execute(
                "
                INSERT INTO sources (source_uuid, source_name, platform, created_at_utc)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(source_uuid) DO UPDATE SET
                    source_name = excluded.source_name,
                    platform = excluded.platform
                ",
                libsql::params![
                    change.source_uuid.clone(),
                    change.source_name.clone(),
                    change.platform.clone(),
                    change.created_at_utc.clone()
                ],
            )
            .await?;
            let revision = Self::insert_revision(
                &tx,
                &change.source_uuid,
                EntityType::Source,
                &change.entity_key(),
            )
            .await?;
            tx.execute(
                "
                INSERT INTO sync_source_changes (
                    revision, source_uuid, source_name, platform, created_at_utc
                ) VALUES (?1, ?2, ?3, ?4, ?5)
                ",
                libsql::params![
                    revision,
                    change.source_uuid.clone(),
                    change.source_name.clone(),
                    change.platform.clone(),
                    change.created_at_utc.clone()
                ],
            )
            .await?;
            applied_revision = revision;
        }

        for change in &batch.input_changes {
            if change.source_uuid != batch.source_uuid {
                bail!("push batch contains foreign-source payload");
            }
            tx.execute(
                "
                INSERT INTO input_buckets (
                    source_uuid, bucket_start_utc, bucket_end_utc, local_date, local_hour,
                    timezone_offset_minutes, granularity_minutes, left_clicks, right_clicks,
                    middle_clicks, key_presses, mouse_distance_cm, scroll_vertical_cm, scroll_horizontal_cm
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                ON CONFLICT(source_uuid, bucket_start_utc, granularity_minutes) DO UPDATE SET
                    bucket_end_utc = excluded.bucket_end_utc,
                    local_date = excluded.local_date,
                    local_hour = excluded.local_hour,
                    timezone_offset_minutes = excluded.timezone_offset_minutes,
                    left_clicks = excluded.left_clicks,
                    right_clicks = excluded.right_clicks,
                    middle_clicks = excluded.middle_clicks,
                    key_presses = excluded.key_presses,
                    mouse_distance_cm = excluded.mouse_distance_cm,
                    scroll_vertical_cm = excluded.scroll_vertical_cm,
                    scroll_horizontal_cm = excluded.scroll_horizontal_cm
                ",
                libsql::params![
                    change.source_uuid.clone(),
                    change.bucket_start_utc.clone(),
                    change.bucket_end_utc.clone(),
                    change.local_date.clone(),
                    change.local_hour as i64,
                    change.timezone_offset_minutes as i64,
                    change.granularity_minutes as i64,
                    change.left_clicks as i64,
                    change.right_clicks as i64,
                    change.middle_clicks as i64,
                    change.key_presses as i64,
                    change.mouse_distance_cm,
                    change.scroll_vertical_cm,
                    change.scroll_horizontal_cm
                ],
            )
            .await?;
            let revision = Self::insert_revision(
                &tx,
                &change.source_uuid,
                EntityType::InputBucket,
                &change.entity_key(),
            )
            .await?;
            tx.execute(
                "
                INSERT INTO sync_input_changes (
                    revision, source_uuid, bucket_start_utc, bucket_end_utc, local_date, local_hour,
                    timezone_offset_minutes, granularity_minutes, left_clicks, right_clicks,
                    middle_clicks, key_presses, mouse_distance_cm, scroll_vertical_cm,
                    scroll_horizontal_cm
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                ",
                libsql::params![
                    revision,
                    change.source_uuid.clone(),
                    change.bucket_start_utc.clone(),
                    change.bucket_end_utc.clone(),
                    change.local_date.clone(),
                    change.local_hour as i64,
                    change.timezone_offset_minutes as i64,
                    change.granularity_minutes as i64,
                    change.left_clicks as i64,
                    change.right_clicks as i64,
                    change.middle_clicks as i64,
                    change.key_presses as i64,
                    change.mouse_distance_cm,
                    change.scroll_vertical_cm,
                    change.scroll_horizontal_cm
                ],
            )
            .await?;
            applied_revision = revision;
        }

        for change in &batch.focus_changes {
            if change.source_uuid != batch.source_uuid {
                bail!("push batch contains foreign-source payload");
            }
            tx.execute(
                "
                INSERT INTO focus_buckets (
                    source_uuid, bucket_start_utc, bucket_end_utc, local_date, local_hour,
                    timezone_offset_minutes, app_identifier, window_title, window_class, focus_seconds
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(source_uuid, bucket_start_utc, window_title, window_class) DO UPDATE SET
                    bucket_end_utc = excluded.bucket_end_utc,
                    local_date = excluded.local_date,
                    local_hour = excluded.local_hour,
                    timezone_offset_minutes = excluded.timezone_offset_minutes,
                    app_identifier = excluded.app_identifier,
                    focus_seconds = excluded.focus_seconds
                ",
                libsql::params![
                    change.source_uuid.clone(),
                    change.bucket_start_utc.clone(),
                    change.bucket_end_utc.clone(),
                    change.local_date.clone(),
                    change.local_hour as i64,
                    change.timezone_offset_minutes as i64,
                    change.app_identifier.clone(),
                    change.window_title.clone(),
                    change.window_class.clone(),
                    change.focus_seconds as i64
                ],
            )
            .await?;
            let revision = Self::insert_revision(
                &tx,
                &change.source_uuid,
                EntityType::FocusBucket,
                &change.entity_key(),
            )
            .await?;
            tx.execute(
                "
                INSERT INTO sync_focus_changes (
                    revision, source_uuid, bucket_start_utc, bucket_end_utc, local_date, local_hour,
                    timezone_offset_minutes, app_identifier, window_title, window_class, focus_seconds
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ",
                libsql::params![
                    revision,
                    change.source_uuid.clone(),
                    change.bucket_start_utc.clone(),
                    change.bucket_end_utc.clone(),
                    change.local_date.clone(),
                    change.local_hour as i64,
                    change.timezone_offset_minutes as i64,
                    change.app_identifier.clone(),
                    change.window_title.clone(),
                    change.window_class.clone(),
                    change.focus_seconds as i64
                ],
            )
            .await?;
            applied_revision = revision;
        }

        tx.execute(
            "
            INSERT INTO sync_applied_batches (batch_uuid, source_uuid, applied_revision, applied_at_utc)
            VALUES (?1, ?2, ?3, ?4)
            ",
            libsql::params![
                batch.batch_uuid.clone(),
                batch.source_uuid.clone(),
                applied_revision,
                Utc::now().to_rfc3339()
            ],
        )
        .await?;
        tx.commit().await?;

        Ok(PushAck {
            applied_revision,
            remote_head_revision: applied_revision,
        })
    }

    async fn pull_since(
        &self,
        own_source_uuid: &str,
        last_pulled_revision: i64,
    ) -> Result<PullResponse> {
        let conn = self.database.as_ref().connect()?;
        let mut rows = conn
            .query(
                "
                SELECT revision, entity_type, source_uuid
                FROM sync_revisions
                WHERE revision > ?1
                ORDER BY revision ASC
                ",
                libsql::params![last_pulled_revision],
            )
            .await?;

        let mut changes = Vec::new();
        while let Some(row) = rows.next().await? {
            let revision = *row.get_value(0)?.as_integer().unwrap_or(&0);
            let entity_type = row
                .get_value(1)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default();
            let source_uuid = row
                .get_value(2)?
                .as_text()
                .map(ToString::to_string)
                .unwrap_or_default();
            if last_pulled_revision == 0 && source_uuid == own_source_uuid {
                continue;
            }
            let payload = Self::load_change_by_revision(&conn, revision, &entity_type).await?;
            changes.push(RemoteChange { revision, payload });
        }

        Ok(PullResponse {
            remote_head_revision: self.remote_head_revision().await?,
            changes,
        })
    }

    async fn status(&self) -> Result<RemoteStatus> {
        Ok(RemoteStatus {
            remote_head_revision: self.remote_head_revision().await?,
        })
    }
}
