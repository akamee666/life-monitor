use anyhow::Result;
use rusqlite::Connection;

use super::outbox::apply_remote_change;
use super::remote::SyncRemote;
use super::state::{
    clear_sync_error, load_sync_state, record_sync_error, record_sync_pull_success,
    SyncRuntimeConfig,
};
use super::types::PullResponse;

#[derive(Debug, Clone)]
pub struct PreparedPull {
    pub last_pulled_revision: i64,
}

pub fn prepare_sync_pull(conn: &Connection, config: &SyncRuntimeConfig) -> Result<PreparedPull> {
    let state = load_sync_state(conn, &config.own_source_uuid, &config.remote_url)?;
    Ok(PreparedPull {
        last_pulled_revision: state.last_pulled_revision,
    })
}

pub fn apply_pull_response(
    conn: &mut Connection,
    config: &SyncRuntimeConfig,
    previous_revision: i64,
    response: &PullResponse,
) -> Result<()> {
    let tx = conn.transaction()?;
    for change in &response.changes {
        apply_remote_change(&tx, &change.payload)?;
    }
    tx.commit()?;

    let last_revision = response
        .changes
        .last()
        .map(|change| change.revision)
        .unwrap_or(previous_revision);
    record_sync_pull_success(
        conn,
        &config.own_source_uuid,
        &config.remote_url,
        response.remote_head_revision.max(last_revision),
        response.remote_head_revision,
    )?;
    clear_sync_error(conn, &config.own_source_uuid, &config.remote_url)?;

    Ok(())
}

pub fn apply_pull_error(
    conn: &Connection,
    config: &SyncRuntimeConfig,
    err: &anyhow::Error,
) -> Result<()> {
    record_sync_error(
        conn,
        &config.own_source_uuid,
        &config.remote_url,
        &err.to_string(),
    )
}

pub async fn sync_pull(
    conn: &mut Connection,
    remote: &(impl SyncRemote + ?Sized),
    config: &SyncRuntimeConfig,
) -> Result<PullResponse> {
    let prepared = prepare_sync_pull(conn, config)?;
    let response = match remote
        .pull_since(&config.own_source_uuid, prepared.last_pulled_revision)
        .await
    {
        Ok(response) => response,
        Err(err) => {
            apply_pull_error(conn, config, &err)?;
            return Err(err);
        }
    };

    apply_pull_response(conn, config, prepared.last_pulled_revision, &response)?;
    Ok(response)
}
