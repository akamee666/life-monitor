use anyhow::Result;
use rusqlite::Connection;

use super::outbox::{mark_batch_sent, prepare_pending_batch, seed_outbox_for_owned_rows};
use super::remote::SyncRemote;
use super::state::{
    clear_sync_error, load_sync_state, record_sync_error, record_sync_push_success,
    SyncRuntimeConfig,
};
use super::types::{PushAck, PushBatch};

#[derive(Debug, Clone)]
pub struct PreparedPush {
    pub batch_uuid: String,
    pub batch: PushBatch,
}

pub fn prepare_sync_push(
    conn: &Connection,
    config: &SyncRuntimeConfig,
) -> Result<Option<PreparedPush>> {
    let state = load_sync_state(conn, &config.own_source_uuid, &config.remote_url)?;
    if state.last_pushed_batch_uuid.is_none() {
        seed_outbox_for_owned_rows(conn, &config.own_source_uuid)?;
    }

    let Some((batch_uuid, entries)) = prepare_pending_batch(conn, &config.own_source_uuid)? else {
        return Ok(None);
    };
    let mut source_changes = Vec::new();
    let mut input_changes = Vec::new();
    let mut focus_changes = Vec::new();
    for entry in entries {
        match entry.payload {
            super::types::ChangePayload::Source(change) => source_changes.push(change),
            super::types::ChangePayload::InputBucket(change) => input_changes.push(change),
            super::types::ChangePayload::FocusBucket(change) => focus_changes.push(change),
        }
    }

    Ok(Some(PreparedPush {
        batch_uuid: batch_uuid.clone(),
        batch: PushBatch {
            batch_uuid,
            source_uuid: config.own_source_uuid.clone(),
            source_changes,
            input_changes,
            focus_changes,
        },
    }))
}

pub fn apply_push_success(
    conn: &Connection,
    config: &SyncRuntimeConfig,
    batch_uuid: &str,
    ack: &PushAck,
) -> Result<()> {
    mark_batch_sent(conn, batch_uuid)?;
    record_sync_push_success(
        conn,
        &config.own_source_uuid,
        &config.remote_url,
        batch_uuid,
        ack.remote_head_revision,
    )?;
    clear_sync_error(conn, &config.own_source_uuid, &config.remote_url)?;
    Ok(())
}

pub async fn sync_push<R: SyncRemote + ?Sized>(
    conn: &Connection,
    remote: &R,
    config: &SyncRuntimeConfig,
) -> Result<Option<PushAck>> {
    let Some(prepared) = prepare_sync_push(conn, config)? else {
        return Ok(None);
    };

    match remote.push_batch(prepared.batch).await {
        Ok(ack) => {
            apply_push_success(conn, config, &prepared.batch_uuid, &ack)?;
            Ok(Some(ack))
        }
        Err(err) => {
            record_sync_error(
                conn,
                &config.own_source_uuid,
                &config.remote_url,
                &err.to_string(),
            )?;
            Err(err)
        }
    }
}
