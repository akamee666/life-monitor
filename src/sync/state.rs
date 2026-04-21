use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::storage::localdb::get_source;

use super::types::SyncStateRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRuntimeConfig {
    pub remote_url: String,
    pub auth_token: String,
    pub own_source_uuid: String,
    pub sync_enabled: bool,
    pub sync_interval_seconds: u64,
}

pub fn resolve_sync_runtime_config(
    conn: &Connection,
    sync_remote_url: Option<&str>,
    sync_auth_token: Option<&str>,
    sync_enable: bool,
    sync_interval: u64,
) -> Result<Option<SyncRuntimeConfig>> {
    let own_source = get_source(conn, crate::common::DEFAULT_SOURCE_ID)?;

    let explicit_remote_url = sync_remote_url.map(str::to_string).or_else(|| {
        std::env::var("LIFE_MONITOR_SYNC_REMOTE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
    });
    let auth_token = sync_auth_token
        .map(str::to_string)
        .or_else(|| std::env::var("LIFE_MONITOR_SYNC_AUTH_TOKEN").ok())
        .unwrap_or_default();

    let existing = explicit_remote_url
        .as_deref()
        .map(|url| load_or_init_sync_state(conn, &own_source.source_uuid, url, sync_enable))
        .transpose()?;

    let persisted = if existing.is_none() {
        conn.query_row(
            "
            SELECT remote_url, sync_enabled
            FROM sync_state
            WHERE own_source_uuid = ?1
            ORDER BY sync_enabled DESC, rowid ASC
            LIMIT 1
            ",
            [own_source.source_uuid.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0)),
        )
        .optional()?
    } else {
        None
    };

    let state_enabled = existing
        .as_ref()
        .map(|state| state.sync_enabled)
        .or_else(|| persisted.as_ref().map(|(_, enabled)| *enabled))
        .unwrap_or(false);
    let remote_url = explicit_remote_url.or_else(|| persisted.as_ref().map(|(url, _)| url.clone()));
    let remote_url = match remote_url {
        Some(url) => url,
        None => return Ok(None),
    };

    let sync_enabled = if sync_enable { true } else { state_enabled };

    if existing.is_none() {
        let _ = load_or_init_sync_state(conn, &own_source.source_uuid, &remote_url, sync_enabled)?;
    }

    Ok(Some(SyncRuntimeConfig {
        remote_url,
        auth_token,
        own_source_uuid: own_source.source_uuid,
        sync_enabled,
        sync_interval_seconds: sync_interval,
    }))
}

pub fn load_or_init_sync_state(
    conn: &Connection,
    own_source_uuid: &str,
    remote_url: &str,
    sync_enabled: bool,
) -> Result<SyncStateRecord> {
    let existing = conn
        .query_row(
            "
            SELECT
                own_source_uuid,
                remote_url,
                last_pulled_revision,
                last_pushed_batch_uuid,
                last_push_at_utc,
                last_pull_at_utc,
                last_sync_error,
                last_sync_error_at_utc,
                remote_head_revision,
                sync_enabled
            FROM sync_state
            WHERE own_source_uuid = ?1 AND remote_url = ?2
            ",
            params![own_source_uuid, remote_url],
            |row| {
                Ok(SyncStateRecord {
                    own_source_uuid: row.get(0)?,
                    remote_url: row.get(1)?,
                    last_pulled_revision: row.get(2)?,
                    last_pushed_batch_uuid: row.get(3)?,
                    last_push_at_utc: row.get(4)?,
                    last_pull_at_utc: row.get(5)?,
                    last_sync_error: row.get(6)?,
                    last_sync_error_at_utc: row.get(7)?,
                    remote_head_revision: row.get(8)?,
                    sync_enabled: row.get::<_, i64>(9)? != 0,
                })
            },
        )
        .optional()?;

    if let Some(mut state) = existing {
        conn.execute(
            "UPDATE sync_state SET sync_enabled = ?3 WHERE own_source_uuid = ?1 AND remote_url = ?2",
            params![own_source_uuid, remote_url, if sync_enabled { 1 } else { 0 }],
        )?;
        state.sync_enabled = sync_enabled;
        return Ok(state);
    }

    conn.execute(
        "
        INSERT INTO sync_state (
            own_source_uuid,
            remote_url,
            last_pulled_revision,
            sync_enabled
        ) VALUES (?1, ?2, 0, ?3)
        ",
        params![
            own_source_uuid,
            remote_url,
            if sync_enabled { 1 } else { 0 }
        ],
    )?;

    Ok(SyncStateRecord {
        own_source_uuid: own_source_uuid.to_string(),
        remote_url: remote_url.to_string(),
        last_pulled_revision: 0,
        last_pushed_batch_uuid: None,
        last_push_at_utc: None,
        last_pull_at_utc: None,
        last_sync_error: None,
        last_sync_error_at_utc: None,
        remote_head_revision: None,
        sync_enabled,
    })
}

pub fn load_sync_state(
    conn: &Connection,
    own_source_uuid: &str,
    remote_url: &str,
) -> Result<SyncStateRecord> {
    conn.query_row(
        "
        SELECT
            own_source_uuid,
            remote_url,
            last_pulled_revision,
            last_pushed_batch_uuid,
            last_push_at_utc,
            last_pull_at_utc,
            last_sync_error,
            last_sync_error_at_utc,
            remote_head_revision,
            sync_enabled
        FROM sync_state
        WHERE own_source_uuid = ?1 AND remote_url = ?2
        ",
        params![own_source_uuid, remote_url],
        |row| {
            Ok(SyncStateRecord {
                own_source_uuid: row.get(0)?,
                remote_url: row.get(1)?,
                last_pulled_revision: row.get(2)?,
                last_pushed_batch_uuid: row.get(3)?,
                last_push_at_utc: row.get(4)?,
                last_pull_at_utc: row.get(5)?,
                last_sync_error: row.get(6)?,
                last_sync_error_at_utc: row.get(7)?,
                remote_head_revision: row.get(8)?,
                sync_enabled: row.get::<_, i64>(9)? != 0,
            })
        },
    )
    .with_context(|| {
        format!(
            "Missing sync state for source {} and remote {}",
            own_source_uuid, remote_url
        )
    })
}

pub fn record_sync_error(
    conn: &Connection,
    own_source_uuid: &str,
    remote_url: &str,
    err: &str,
) -> Result<()> {
    conn.execute(
        "
        UPDATE sync_state
        SET last_sync_error = ?3, last_sync_error_at_utc = ?4
        WHERE own_source_uuid = ?1 AND remote_url = ?2
        ",
        params![own_source_uuid, remote_url, err, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub fn clear_sync_error(conn: &Connection, own_source_uuid: &str, remote_url: &str) -> Result<()> {
    conn.execute(
        "
        UPDATE sync_state
        SET last_sync_error = NULL, last_sync_error_at_utc = NULL
        WHERE own_source_uuid = ?1 AND remote_url = ?2
        ",
        params![own_source_uuid, remote_url],
    )?;
    Ok(())
}

pub fn record_sync_push_success(
    conn: &Connection,
    own_source_uuid: &str,
    remote_url: &str,
    batch_uuid: &str,
    remote_head_revision: i64,
) -> Result<()> {
    conn.execute(
        "
        UPDATE sync_state
        SET
            last_pushed_batch_uuid = ?3,
            last_push_at_utc = ?4,
            remote_head_revision = ?5,
            last_sync_error = NULL,
            last_sync_error_at_utc = NULL
        WHERE own_source_uuid = ?1 AND remote_url = ?2
        ",
        params![
            own_source_uuid,
            remote_url,
            batch_uuid,
            Utc::now().to_rfc3339(),
            remote_head_revision
        ],
    )?;
    Ok(())
}

pub fn record_sync_pull_success(
    conn: &Connection,
    own_source_uuid: &str,
    remote_url: &str,
    last_pulled_revision: i64,
    remote_head_revision: i64,
) -> Result<()> {
    conn.execute(
        "
        UPDATE sync_state
        SET
            last_pulled_revision = ?3,
            last_pull_at_utc = ?4,
            remote_head_revision = ?5,
            last_sync_error = NULL,
            last_sync_error_at_utc = NULL
        WHERE own_source_uuid = ?1 AND remote_url = ?2
        ",
        params![
            own_source_uuid,
            remote_url,
            last_pulled_revision,
            Utc::now().to_rfc3339(),
            remote_head_revision
        ],
    )?;
    Ok(())
}
