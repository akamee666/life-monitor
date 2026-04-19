use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

use super::outbox::list_pending_outbox;
use super::remote::SyncRemote;
use super::state::{load_sync_state, SyncRuntimeConfig};
use super::types::SyncStatusSnapshot;

pub async fn sync_status_snapshot(
    conn: &Connection,
    remote: Option<&(impl SyncRemote + ?Sized)>,
    config: &SyncRuntimeConfig,
) -> Result<SyncStatusSnapshot> {
    let state = load_sync_state(conn, &config.own_source_uuid, &config.remote_url)?;
    let pending_outbox_count = list_pending_outbox(conn, &config.own_source_uuid)?.len() as u64;
    let remote_status = match remote {
        Some(remote) => remote.status().await.ok(),
        None => None,
    };

    let remote_head_revision = remote_status
        .map(|status| status.remote_head_revision)
        .or(state.remote_head_revision);
    let last_push_age_seconds = age_seconds(state.last_push_at_utc.as_deref());
    let last_pull_age_seconds = age_seconds(state.last_pull_at_utc.as_deref());

    Ok(SyncStatusSnapshot {
        sync_enabled: state.sync_enabled,
        remote_url: state.remote_url,
        own_source_uuid: state.own_source_uuid,
        last_push_at_utc: state.last_push_at_utc,
        last_pull_at_utc: state.last_pull_at_utc,
        last_push_age_seconds,
        last_pull_age_seconds,
        remote_head_revision,
        last_pulled_revision: state.last_pulled_revision,
        pending_outbox_count,
        last_sync_error: state.last_sync_error,
        last_sync_error_at_utc: state.last_sync_error_at_utc,
        is_caught_up: remote_head_revision.map(|head| head <= state.last_pulled_revision),
    })
}

pub fn render_sync_status(status: &SyncStatusSnapshot) -> String {
    [
        format!("sync enabled: {}", status.sync_enabled),
        format!("remote url: {}", status.remote_url),
        format!("own source uuid: {}", status.own_source_uuid),
        format!(
            "last push: {}",
            status.last_push_at_utc.as_deref().unwrap_or("never")
        ),
        format!(
            "last push age seconds: {}",
            status
                .last_push_age_seconds
                .map(|age| age.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ),
        format!(
            "last pull: {}",
            status.last_pull_at_utc.as_deref().unwrap_or("never")
        ),
        format!(
            "last pull age seconds: {}",
            status
                .last_pull_age_seconds
                .map(|age| age.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ),
        format!(
            "remote head revision: {}",
            status
                .remote_head_revision
                .map(|rev| rev.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ),
        format!("local applied revision: {}", status.last_pulled_revision),
        format!("pending outbox count: {}", status.pending_outbox_count),
        format!(
            "caught up: {}",
            status
                .is_caught_up
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ),
        format!(
            "last sync error: {}",
            status.last_sync_error.as_deref().unwrap_or("none")
        ),
        format!(
            "last sync error at: {}",
            status.last_sync_error_at_utc.as_deref().unwrap_or("never")
        ),
    ]
    .join("\n")
}

fn age_seconds(value: Option<&str>) -> Option<i64> {
    let value = value?;
    let parsed = chrono::DateTime::parse_from_rfc3339(value).ok()?;
    Some(
        (Utc::now() - parsed.with_timezone(&Utc))
            .num_seconds()
            .max(0),
    )
}
