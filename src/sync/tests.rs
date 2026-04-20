use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};
use rusqlite::Connection;
use uuid::Uuid;

use crate::common::{FocusBucketRecord, InputBucketRecord, DEFAULT_SOURCE_ID};
use crate::storage::localdb::{get_source, open_con_at, setup_database};

use super::outbox::{apply_local_focus_rows, apply_local_input_rows, list_pending_outbox};
use super::pull::sync_pull;
use super::push::{prepare_sync_push, sync_push};
use super::remote::{InMemoryRemote, SyncRemote};
use super::state::{
    load_or_init_sync_state, load_sync_state, record_sync_pull_success, SyncRuntimeConfig,
};
use super::status::sync_status_snapshot;
use super::types::{
    FocusBucketChange, PullResponse, PushAck, PushBatch, RemoteChange, RemoteStatus,
};

fn unique_temp_db(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("life-monitor-sync-{name}-{}.db", Uuid::new_v4()))
}

fn build_test_db(path: &Path) -> Result<Connection> {
    let conn = open_con_at(path)?;
    setup_database(&conn)?;
    Ok(conn)
}

fn own_source_uuid(conn: &Connection) -> Result<String> {
    Ok(get_source(conn, DEFAULT_SOURCE_ID)?.source_uuid)
}

fn sync_config(conn: &Connection, remote_url: &str) -> Result<SyncRuntimeConfig> {
    let own_source_uuid = own_source_uuid(conn)?;
    load_or_init_sync_state(conn, &own_source_uuid, remote_url, true)?;
    Ok(SyncRuntimeConfig {
        remote_url: remote_url.to_string(),
        auth_token: String::new(),
        own_source_uuid,
        sync_enabled: true,
        sync_interval_seconds: 300,
    })
}

fn sample_input_row(source_id: i64, minute: u32, key_presses: u64) -> InputBucketRecord {
    InputBucketRecord {
        source_id,
        bucket_start_utc: Utc.with_ymd_and_hms(2026, 4, 19, 12, minute, 0).unwrap(),
        bucket_end_utc: Utc
            .with_ymd_and_hms(2026, 4, 19, 12, minute + 15, 0)
            .unwrap(),
        local_date: "2026-04-19".to_string(),
        local_hour: 9,
        timezone_offset_minutes: -180,
        granularity_minutes: 15,
        left_clicks: 2,
        right_clicks: 1,
        middle_clicks: 0,
        key_presses,
        mouse_distance_cm: 3.0,
        scroll_vertical_cm: 0.4,
        scroll_horizontal_cm: 0.0,
    }
}

fn sample_focus_row(source_id: i64, minute: u32, title: &str) -> FocusBucketRecord {
    FocusBucketRecord {
        source_id,
        bucket_start_utc: Utc.with_ymd_and_hms(2026, 4, 19, 12, minute, 0).unwrap(),
        bucket_end_utc: Utc
            .with_ymd_and_hms(2026, 4, 19, 12, minute + 15, 0)
            .unwrap(),
        local_date: "2026-04-19".to_string(),
        local_hour: 9,
        timezone_offset_minutes: -180,
        app_identifier: "firefox".to_string(),
        window_title: title.to_string(),
        window_class: "firefox".to_string(),
        focus_seconds: 120,
    }
}

fn input_snapshot(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "
        SELECT s.source_uuid, i.bucket_start_utc, i.key_presses, i.left_clicks, i.mouse_distance_cm
        FROM input_buckets i
        JOIN sources s ON s.id = i.source_id
        ORDER BY s.source_uuid, i.bucket_start_utc
        ",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(format!(
            "{}|{}|{}|{}|{}",
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, u64>(2)?,
            row.get::<_, u64>(3)?,
            row.get::<_, f64>(4)?,
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn focus_snapshot(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "
        SELECT s.source_uuid, f.bucket_start_utc, f.window_title, f.focus_seconds
        FROM focus_buckets f
        JOIN sources s ON s.id = f.source_id
        ORDER BY s.source_uuid, f.bucket_start_utc, f.window_title
        ",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(format!(
            "{}|{}|{}|{}",
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, u64>(3)?,
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn source_snapshot(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "
        SELECT source_uuid, source_name, platform
        FROM sources
        ORDER BY source_uuid
        ",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(format!(
            "{}|{}|{}",
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

struct AlwaysFailRemote;

impl SyncRemote for AlwaysFailRemote {
    async fn push_batch(&self, _batch: PushBatch) -> Result<PushAck> {
        Err(anyhow!("remote unavailable"))
    }

    async fn pull_since(
        &self,
        _own_source_uuid: &str,
        _last_pulled_revision: i64,
    ) -> Result<PullResponse> {
        Err(anyhow!("remote unavailable"))
    }

    async fn status(&self) -> Result<RemoteStatus> {
        Err(anyhow!("remote unavailable"))
    }
}

struct FailOnceRemote {
    inner: InMemoryRemote,
    failures_remaining: AtomicUsize,
}

impl FailOnceRemote {
    fn new() -> Self {
        Self {
            inner: InMemoryRemote::default(),
            failures_remaining: AtomicUsize::new(1),
        }
    }
}

impl SyncRemote for FailOnceRemote {
    async fn push_batch(&self, batch: PushBatch) -> Result<PushAck> {
        if self
            .failures_remaining
            .compare_exchange(1, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return Err(anyhow!("transient push failure"));
        }
        self.inner.push_batch(batch).await
    }

    async fn pull_since(
        &self,
        own_source_uuid: &str,
        last_pulled_revision: i64,
    ) -> Result<PullResponse> {
        self.inner
            .pull_since(own_source_uuid, last_pulled_revision)
            .await
    }

    async fn status(&self) -> Result<RemoteStatus> {
        self.inner.status().await
    }
}

struct StaticPullRemote {
    response: PullResponse,
}

impl SyncRemote for StaticPullRemote {
    async fn push_batch(&self, _batch: PushBatch) -> Result<PushAck> {
        Err(anyhow!("push not supported"))
    }

    async fn pull_since(
        &self,
        _own_source_uuid: &str,
        _last_pulled_revision: i64,
    ) -> Result<PullResponse> {
        Ok(self.response.clone())
    }

    async fn status(&self) -> Result<RemoteStatus> {
        Ok(RemoteStatus {
            remote_head_revision: self.response.remote_head_revision,
        })
    }
}

/// Proves local bucket writes enqueue exactly the owned rows that sync will later push.
/// It uses real SQLite writes and inspects only persisted outbox state, which keeps it stable.
/// This catches regressions where local collection stops queuing sync work or queues duplicates.
#[tokio::test]
async fn outbox_tracks_local_rows_for_owned_bucket_writes() -> Result<()> {
    let path = unique_temp_db("outbox");
    let conn = build_test_db(&path)?;
    let config = sync_config(&conn, "memory://outbox")?;

    apply_local_input_rows(&conn, &[sample_input_row(DEFAULT_SOURCE_ID, 0, 9)])?;
    apply_local_focus_rows(&conn, &[sample_focus_row(DEFAULT_SOURCE_ID, 0, "Docs")])?;

    let pending = list_pending_outbox(&conn, &config.own_source_uuid)?;
    assert_eq!(pending.len(), 2);
    assert!(pending
        .iter()
        .any(|entry| matches!(entry.payload, super::types::ChangePayload::InputBucket(_))));
    assert!(pending
        .iter()
        .any(|entry| matches!(entry.payload, super::types::ChangePayload::FocusBucket(_))));

    Ok(())
}

/// Proves the same prepared batch can be replayed without duplicating canonical remote rows.
/// It uses a deterministic in-process remote and compares visible revisions, not internals.
/// This catches regressions in batch idempotency and outbox acknowledgement ordering.
#[tokio::test]
async fn push_replay_is_idempotent_and_ack_clears_pending_rows() -> Result<()> {
    let path = unique_temp_db("push-idempotent");
    let conn = build_test_db(&path)?;
    let config = sync_config(&conn, "memory://push-idempotent")?;
    let remote = InMemoryRemote::default();

    apply_local_input_rows(&conn, &[sample_input_row(DEFAULT_SOURCE_ID, 0, 9)])?;
    apply_local_focus_rows(&conn, &[sample_focus_row(DEFAULT_SOURCE_ID, 0, "Docs")])?;

    let prepared = prepare_sync_push(&conn, &config)?.expect("pending batch");
    let ack1 = remote.push_batch(prepared.batch.clone()).await?;
    let ack2 = remote.push_batch(prepared.batch.clone()).await?;

    assert_eq!(ack1, ack2);

    let sent = sync_push(&conn, &remote, &config).await?.expect("push ack");
    assert_eq!(sent.remote_head_revision, 3);
    assert!(list_pending_outbox(&conn, &config.own_source_uuid)?.is_empty());
    assert_eq!(remote.status().await?.remote_head_revision, 3);
    assert!(sync_push(&conn, &remote, &config).await?.is_none());

    Ok(())
}

/// Proves pull only applies revisions above the current cursor and advances the cursor after success.
/// It uses two real SQLite databases plus an in-process remote, so there is no timing or network noise.
/// This catches duplicate application, skipped rows, and incorrect cursor advancement.
#[tokio::test]
async fn incremental_pull_applies_only_new_revisions_and_advances_cursor_on_success() -> Result<()>
{
    let source_path = unique_temp_db("pull-source");
    let target_path = unique_temp_db("pull-target");
    let source = build_test_db(&source_path)?;
    let mut target = build_test_db(&target_path)?;
    let source_config = sync_config(&source, "memory://pull")?;
    let target_config = sync_config(&target, "memory://pull")?;
    let remote = InMemoryRemote::default();

    apply_local_input_rows(&source, &[sample_input_row(DEFAULT_SOURCE_ID, 0, 5)])?;
    sync_push(&source, &remote, &source_config).await?;

    let first = sync_pull(&mut target, &remote, &target_config).await?;
    assert_eq!(first.changes.len(), 2);
    assert_eq!(input_snapshot(&target)?.len(), 1);
    assert_eq!(
        load_sync_state(
            &target,
            &target_config.own_source_uuid,
            &target_config.remote_url
        )?
        .last_pulled_revision,
        2
    );

    apply_local_input_rows(&source, &[sample_input_row(DEFAULT_SOURCE_ID, 15, 7)])?;
    sync_push(&source, &remote, &source_config).await?;

    let second = sync_pull(&mut target, &remote, &target_config).await?;
    assert_eq!(second.changes.len(), 2);
    assert_eq!(input_snapshot(&target)?.len(), 2);
    assert_eq!(
        load_sync_state(
            &target,
            &target_config.own_source_uuid,
            &target_config.remote_url
        )?
        .last_pulled_revision,
        3
    );

    Ok(())
}

/// Proves a failed pull leaves the cursor unchanged so the same revision range can be retried safely.
/// It injects invalid payload data instead of relying on I/O failures, which keeps the test deterministic.
/// This catches partial-apply bugs that would silently skip remote history after one bad pull.
#[tokio::test]
async fn pull_failure_does_not_advance_cursor() -> Result<()> {
    let path = unique_temp_db("pull-failure");
    let mut conn = build_test_db(&path)?;
    let config = sync_config(&conn, "memory://pull-failure")?;
    let foreign_uuid = Uuid::new_v4().to_string();
    let remote = StaticPullRemote {
        response: PullResponse {
            remote_head_revision: 1,
            changes: vec![RemoteChange {
                revision: 1,
                payload: super::types::ChangePayload::FocusBucket(FocusBucketChange {
                    source_uuid: foreign_uuid,
                    bucket_start_utc: "not-a-timestamp".to_string(),
                    bucket_end_utc: "still-not-a-timestamp".to_string(),
                    local_date: "2026-04-19".to_string(),
                    local_hour: 9,
                    timezone_offset_minutes: -180,
                    app_identifier: "firefox".to_string(),
                    window_title: "Broken".to_string(),
                    window_class: "firefox".to_string(),
                    focus_seconds: 30,
                }),
            }],
        },
    };

    assert!(sync_pull(&mut conn, &remote, &config).await.is_err());
    assert_eq!(
        load_sync_state(&conn, &config.own_source_uuid, &config.remote_url)?.last_pulled_revision,
        0
    );

    Ok(())
}

/// Proves replicated foreign rows remain readable locally but are not re-enqueued as local authored work.
/// It checks persisted local state after a full push/pull cycle, which makes the assertion stable.
/// This catches convergence loops where pulled rows bounce back into the local outbox.
#[tokio::test]
async fn foreign_rows_are_pulled_without_entering_the_outbox() -> Result<()> {
    let source_path = unique_temp_db("ownership-source");
    let target_path = unique_temp_db("ownership-target");
    let source = build_test_db(&source_path)?;
    let mut target = build_test_db(&target_path)?;
    let source_config = sync_config(&source, "memory://ownership")?;
    let target_config = sync_config(&target, "memory://ownership")?;
    let remote = InMemoryRemote::default();

    apply_local_input_rows(&source, &[sample_input_row(DEFAULT_SOURCE_ID, 0, 11)])?;
    sync_push(&source, &remote, &source_config).await?;
    sync_pull(&mut target, &remote, &target_config).await?;

    assert_eq!(input_snapshot(&target)?.len(), 1);
    assert!(list_pending_outbox(&target, &target_config.own_source_uuid)?.is_empty());

    Ok(())
}

/// Proves pull injects real source metadata before foreign bucket rows are applied, instead of
/// synthesizing placeholder local source records with guessed names or platforms.
/// It is stable because the source metadata is set explicitly in the source database and then
/// verified in the pulled target database after one push/pull cycle.
/// This catches regressions where foreign rows create fake local source entries during apply.
#[tokio::test]
async fn pull_applies_real_foreign_source_metadata_before_bucket_rows() -> Result<()> {
    let source_path = unique_temp_db("foreign-source-metadata-source");
    let target_path = unique_temp_db("foreign-source-metadata-target");
    let source = build_test_db(&source_path)?;
    let mut target = build_test_db(&target_path)?;
    let source_config = sync_config(&source, "memory://foreign-source-metadata")?;
    let target_config = sync_config(&target, "memory://foreign-source-metadata")?;
    let remote = InMemoryRemote::default();

    source.execute(
        "UPDATE sources SET source_name = 'thinkpad', platform = 'windows' WHERE id = ?1",
        [DEFAULT_SOURCE_ID],
    )?;
    super::outbox::apply_local_source(&source, &get_source(&source, DEFAULT_SOURCE_ID)?)?;
    apply_local_input_rows(&source, &[sample_input_row(DEFAULT_SOURCE_ID, 0, 11)])?;

    sync_push(&source, &remote, &source_config).await?;
    sync_pull(&mut target, &remote, &target_config).await?;

    let foreign_source = source_snapshot(&target)?
        .into_iter()
        .find(|row| row.contains("|thinkpad|windows"))
        .expect("foreign source metadata should be replicated");
    assert!(foreign_source.contains("thinkpad"));
    assert!(list_pending_outbox(&target, &target_config.own_source_uuid)?.is_empty());

    Ok(())
}

/// Proves remote ownership validation rejects a batch whose payload rows do not belong to the batch source.
/// It builds the mismatched batch directly in memory, so the failure surface is narrow and deterministic.
/// This catches cross-device overwrite bugs that would corrupt canonical remote state.
#[tokio::test]
async fn push_rejects_foreign_source_mutation() -> Result<()> {
    let path = unique_temp_db("foreign-push");
    let conn = build_test_db(&path)?;
    let config = sync_config(&conn, "memory://foreign-push")?;
    let remote = InMemoryRemote::default();
    let foreign_uuid = Uuid::new_v4().to_string();

    let batch = PushBatch {
        batch_uuid: Uuid::new_v4().to_string(),
        source_uuid: config.own_source_uuid.clone(),
        source_changes: Vec::new(),
        input_changes: vec![super::types::InputBucketChange {
            source_uuid: foreign_uuid,
            bucket_start_utc: "2026-04-19T12:00:00Z".to_string(),
            bucket_end_utc: "2026-04-19T12:15:00Z".to_string(),
            local_date: "2026-04-19".to_string(),
            local_hour: 9,
            timezone_offset_minutes: -180,
            granularity_minutes: 15,
            left_clicks: 1,
            right_clicks: 0,
            middle_clicks: 0,
            key_presses: 1,
            mouse_distance_cm: 1.0,
            scroll_vertical_cm: 0.0,
            scroll_horizontal_cm: 0.0,
        }],
        focus_changes: Vec::new(),
    };

    assert!(remote.push_batch(batch).await.is_err());
    assert!(list_pending_outbox(&conn, &own_source_uuid(&conn)?)?.is_empty());

    Ok(())
}

/// Proves local collection continues and pending sync work is preserved when the remote is unavailable.
/// It uses a deterministic failing remote and inspects only local SQLite state after the failed push.
/// This catches regressions where offline sync attempts drop data or clear the queue too early.
#[tokio::test]
async fn offline_push_keeps_local_data_and_pending_outbox() -> Result<()> {
    let path = unique_temp_db("offline");
    let conn = build_test_db(&path)?;
    let config = sync_config(&conn, "memory://offline")?;

    apply_local_input_rows(&conn, &[sample_input_row(DEFAULT_SOURCE_ID, 0, 3)])?;
    assert!(sync_push(&conn, &AlwaysFailRemote, &config).await.is_err());

    assert_eq!(input_snapshot(&conn)?.len(), 1);
    assert_eq!(
        list_pending_outbox(&conn, &config.own_source_uuid)?.len(),
        2
    );

    Ok(())
}

/// Proves a transient failure followed by retry produces one canonical application and clears the queue once.
/// It uses a fake remote that fails exactly once and then behaves deterministically.
/// This catches duplicate remote application and stuck pending-row regressions.
#[tokio::test]
async fn retry_push_applies_pending_changes_once() -> Result<()> {
    let path = unique_temp_db("retry");
    let conn = build_test_db(&path)?;
    let config = sync_config(&conn, "memory://retry")?;
    let remote = FailOnceRemote::new();

    apply_local_input_rows(&conn, &[sample_input_row(DEFAULT_SOURCE_ID, 0, 8)])?;

    assert!(sync_push(&conn, &remote, &config).await.is_err());
    assert_eq!(
        list_pending_outbox(&conn, &config.own_source_uuid)?.len(),
        2
    );

    let ack = sync_push(&conn, &remote, &config)
        .await?
        .expect("ack after retry");

    assert_eq!(ack.remote_head_revision, 2);
    assert!(list_pending_outbox(&conn, &config.own_source_uuid)?.is_empty());
    assert_eq!(remote.status().await?.remote_head_revision, 2);

    Ok(())
}

/// Proves two devices with different local writes converge to the same canonical dataset after push then pull.
/// It uses two real SQLite databases and one shared in-process remote with no timing races.
/// This catches the core multi-device regression: divergent local state after a nominal sync cycle.
#[tokio::test]
async fn two_devices_converge_after_push_then_pull() -> Result<()> {
    let remote = Arc::new(InMemoryRemote::default());
    let path_a = unique_temp_db("converge-a");
    let path_b = unique_temp_db("converge-b");
    let conn_a = build_test_db(&path_a)?;
    let conn_b = build_test_db(&path_b)?;
    let config_a = sync_config(&conn_a, "memory://converge")?;
    let config_b = sync_config(&conn_b, "memory://converge")?;

    apply_local_input_rows(&conn_a, &[sample_input_row(DEFAULT_SOURCE_ID, 0, 10)])?;
    apply_local_focus_rows(&conn_b, &[sample_focus_row(DEFAULT_SOURCE_ID, 15, "Mail")])?;

    sync_push(&conn_a, remote.as_ref(), &config_a).await?;
    sync_push(&conn_b, remote.as_ref(), &config_b).await?;

    let mut pull_a = open_con_at(&path_a)?;
    let mut pull_b = open_con_at(&path_b)?;
    sync_pull(&mut pull_a, remote.as_ref(), &config_a).await?;
    sync_pull(&mut pull_b, remote.as_ref(), &config_b).await?;

    assert_eq!(input_snapshot(&pull_a)?, input_snapshot(&pull_b)?);
    assert_eq!(focus_snapshot(&pull_a)?, focus_snapshot(&pull_b)?);
    assert_eq!(remote.status().await?.remote_head_revision, 4);

    Ok(())
}

/// Proves status prefers live remote information when the remote is reachable.
/// It uses a deterministic in-memory remote and asserts only on the observable status snapshot.
/// This catches stale-status regressions where the command stops reflecting the current remote head.
#[tokio::test]
async fn sync_status_reports_remote_head_when_remote_is_available() -> Result<()> {
    let path = unique_temp_db("status-online");
    let conn = build_test_db(&path)?;
    let config = sync_config(&conn, "memory://status-online")?;
    let remote = InMemoryRemote::default();

    apply_local_input_rows(&conn, &[sample_input_row(DEFAULT_SOURCE_ID, 0, 6)])?;
    sync_push(&conn, &remote, &config).await?;

    let status = sync_status_snapshot(&conn, Some(&remote), &config).await?;
    assert_eq!(status.remote_head_revision, Some(2));
    assert_eq!(status.pending_outbox_count, 0);
    assert_eq!(status.is_caught_up, Some(false));

    Ok(())
}

/// Proves status still reports the last known remote head when the live remote status call fails.
/// It relies on persisted `sync_state` metadata instead of timing-sensitive retries, so it stays stable.
/// This catches regressions where status becomes useless offline even though previous sync metadata exists.
#[tokio::test]
async fn sync_status_falls_back_to_persisted_remote_head_after_remote_error() -> Result<()> {
    let path = unique_temp_db("status-offline");
    let conn = build_test_db(&path)?;
    let config = sync_config(&conn, "memory://status-offline")?;

    record_sync_pull_success(&conn, &config.own_source_uuid, &config.remote_url, 3, 5)?;

    let status = sync_status_snapshot(&conn, Some(&AlwaysFailRemote), &config).await?;
    assert_eq!(status.remote_head_revision, Some(5));
    assert_eq!(status.last_pulled_revision, 3);
    assert_eq!(status.is_caught_up, Some(false));

    Ok(())
}
