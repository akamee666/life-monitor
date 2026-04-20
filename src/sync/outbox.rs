use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::common::{FocusBucketRecord, InputBucketRecord, SourceInfo};
use crate::storage::localdb::{
    get_source, get_source_by_uuid, insert_focus_buckets, insert_input_buckets,
    upsert_source_by_uuid,
};

use super::types::{
    ChangePayload, EntityType, FocusBucketChange, InputBucketChange, OutboxEntry, SourceChange,
};

pub fn apply_local_source(conn: &Connection, source: &SourceInfo) -> Result<()> {
    let change = SourceChange {
        source_uuid: source.source_uuid.clone(),
        source_name: source.source_name.clone(),
        platform: source.platform.clone(),
        created_at_utc: Utc::now().to_rfc3339(),
    };
    upsert_source_by_uuid(
        conn,
        &change.source_uuid,
        &change.source_name,
        &change.platform,
        &change.created_at_utc,
    )?;
    enqueue_source_change(conn, &change)
}

pub fn apply_local_input_rows(conn: &Connection, rows: &[InputBucketRecord]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    insert_input_buckets(conn, rows)?;
    let source = get_source(conn, rows[0].source_id)?;
    for row in rows {
        enqueue_input_change(conn, &input_change_from_row(row, &source.source_uuid))?;
    }
    Ok(())
}

pub fn apply_local_focus_rows(conn: &Connection, rows: &[FocusBucketRecord]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    insert_focus_buckets(conn, rows)?;
    let source = get_source(conn, rows[0].source_id)?;
    for row in rows {
        enqueue_focus_change(conn, &focus_change_from_row(row, &source.source_uuid))?;
    }
    Ok(())
}

pub fn apply_remote_change(conn: &Connection, change: &ChangePayload) -> Result<()> {
    match change {
        ChangePayload::Source(change) => {
            upsert_source_by_uuid(
                conn,
                &change.source_uuid,
                &change.source_name,
                &change.platform,
                &change.created_at_utc,
            )?;
        }
        ChangePayload::InputBucket(change) => {
            let source_id =
                resolve_remote_source_id(conn, &change.source_uuid, &change.bucket_start_utc)?;
            let row = input_row_from_change(change, source_id)?;
            insert_input_buckets(conn, &[row])?;
        }
        ChangePayload::FocusBucket(change) => {
            let source_id =
                resolve_remote_source_id(conn, &change.source_uuid, &change.bucket_start_utc)?;
            let row = focus_row_from_change(change, source_id)?;
            insert_focus_buckets(conn, &[row])?;
        }
    }
    Ok(())
}

pub fn list_pending_outbox(conn: &Connection, own_source_uuid: &str) -> Result<Vec<OutboxEntry>> {
    let mut entries = Vec::new();

    let mut source_stmt = conn.prepare(
        "
        SELECT id, batch_uuid, source_uuid, source_name, platform, created_at_utc, sent_at_utc, attempt_count
        FROM sync_outbox_sources
        WHERE sent_at_utc IS NULL AND source_uuid = ?1
        ",
    )?;
    let source_rows = source_stmt.query_map([own_source_uuid], |row| {
        let change = SourceChange {
            source_uuid: row.get(2)?,
            source_name: row.get(3)?,
            platform: row.get(4)?,
            created_at_utc: row.get(5)?,
        };
        Ok(OutboxEntry {
            id: row.get(0)?,
            batch_uuid: row.get(1)?,
            entity_type: EntityType::Source,
            entity_key: change.entity_key(),
            source_uuid: change.source_uuid.clone(),
            payload: ChangePayload::Source(change),
            created_at_utc: row.get(5)?,
            sent_at_utc: row.get(6)?,
            attempt_count: row.get::<_, i64>(7)? as u32,
        })
    })?;
    entries.extend(source_rows.collect::<rusqlite::Result<Vec<_>>>()?);

    let mut input_stmt = conn.prepare(
        "
        SELECT id, batch_uuid, source_uuid, bucket_start_utc, bucket_end_utc, local_date,
               local_hour, timezone_offset_minutes, granularity_minutes, left_clicks,
               right_clicks, middle_clicks, key_presses, mouse_distance_cm, scroll_vertical_cm,
               scroll_horizontal_cm, created_at_utc, sent_at_utc, attempt_count
        FROM sync_outbox_input_buckets
        WHERE sent_at_utc IS NULL AND source_uuid = ?1
        ",
    )?;
    let input_rows = input_stmt.query_map([own_source_uuid], |row| {
        let change = InputBucketChange {
            source_uuid: row.get(2)?,
            bucket_start_utc: row.get(3)?,
            bucket_end_utc: row.get(4)?,
            local_date: row.get(5)?,
            local_hour: row.get(6)?,
            timezone_offset_minutes: row.get(7)?,
            granularity_minutes: row.get(8)?,
            left_clicks: row.get(9)?,
            right_clicks: row.get(10)?,
            middle_clicks: row.get(11)?,
            key_presses: row.get(12)?,
            mouse_distance_cm: row.get(13)?,
            scroll_vertical_cm: row.get(14)?,
            scroll_horizontal_cm: row.get(15)?,
        };
        Ok(OutboxEntry {
            id: row.get(0)?,
            batch_uuid: row.get(1)?,
            entity_type: EntityType::InputBucket,
            entity_key: change.entity_key(),
            source_uuid: change.source_uuid.clone(),
            payload: ChangePayload::InputBucket(change),
            created_at_utc: row.get(16)?,
            sent_at_utc: row.get(17)?,
            attempt_count: row.get::<_, i64>(18)? as u32,
        })
    })?;
    entries.extend(input_rows.collect::<rusqlite::Result<Vec<_>>>()?);

    let mut focus_stmt = conn.prepare(
        "
        SELECT id, batch_uuid, source_uuid, bucket_start_utc, bucket_end_utc, local_date,
               local_hour, timezone_offset_minutes, app_identifier, window_title, window_class,
               focus_seconds, created_at_utc, sent_at_utc, attempt_count
        FROM sync_outbox_focus_buckets
        WHERE sent_at_utc IS NULL AND source_uuid = ?1
        ",
    )?;
    let focus_rows = focus_stmt.query_map([own_source_uuid], |row| {
        let change = FocusBucketChange {
            source_uuid: row.get(2)?,
            bucket_start_utc: row.get(3)?,
            bucket_end_utc: row.get(4)?,
            local_date: row.get(5)?,
            local_hour: row.get(6)?,
            timezone_offset_minutes: row.get(7)?,
            app_identifier: row.get(8)?,
            window_title: row.get(9)?,
            window_class: row.get(10)?,
            focus_seconds: row.get(11)?,
        };
        Ok(OutboxEntry {
            id: row.get(0)?,
            batch_uuid: row.get(1)?,
            entity_type: EntityType::FocusBucket,
            entity_key: change.entity_key(),
            source_uuid: change.source_uuid.clone(),
            payload: ChangePayload::FocusBucket(change),
            created_at_utc: row.get(12)?,
            sent_at_utc: row.get(13)?,
            attempt_count: row.get::<_, i64>(14)? as u32,
        })
    })?;
    entries.extend(focus_rows.collect::<rusqlite::Result<Vec<_>>>()?);

    entries.sort_by(|left, right| {
        left.created_at_utc
            .cmp(&right.created_at_utc)
            .then(left.entity_key.cmp(&right.entity_key))
    });

    Ok(entries)
}

pub fn prepare_pending_batch(
    conn: &Connection,
    own_source_uuid: &str,
) -> Result<Option<(String, Vec<OutboxEntry>)>> {
    let existing_batch_uuid: Option<String> = conn
        .query_row(
            "
            SELECT batch_uuid
            FROM (
                SELECT batch_uuid, created_at_utc
                FROM sync_outbox_sources
                WHERE sent_at_utc IS NULL AND source_uuid = ?1 AND batch_uuid IS NOT NULL
                UNION ALL
                SELECT batch_uuid, created_at_utc
                FROM sync_outbox_input_buckets
                WHERE sent_at_utc IS NULL AND source_uuid = ?1 AND batch_uuid IS NOT NULL
                UNION ALL
                SELECT batch_uuid, created_at_utc
                FROM sync_outbox_focus_buckets
                WHERE sent_at_utc IS NULL AND source_uuid = ?1 AND batch_uuid IS NOT NULL
            )
            ORDER BY created_at_utc ASC
            LIMIT 1
            ",
            [own_source_uuid],
            |row| row.get(0),
        )
        .optional()?;

    let batch_uuid = if let Some(batch_uuid) = existing_batch_uuid {
        increment_batch_attempts(conn, own_source_uuid, &batch_uuid)?;
        batch_uuid
    } else {
        if pending_outbox_count(conn, own_source_uuid)? == 0 {
            return Ok(None);
        }

        let batch_uuid = Uuid::new_v4().to_string();
        assign_batch_uuid(conn, own_source_uuid, &batch_uuid)?;
        batch_uuid
    };

    let mut entries = list_pending_outbox(conn, own_source_uuid)?;
    entries.retain(|entry| entry.batch_uuid.as_deref() == Some(batch_uuid.as_str()));
    Ok(Some((batch_uuid, entries)))
}

pub fn mark_batch_sent(conn: &Connection, batch_uuid: &str) -> Result<()> {
    let sent_at = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE sync_outbox_sources SET sent_at_utc = ?2 WHERE batch_uuid = ?1 AND sent_at_utc IS NULL",
        params![batch_uuid, sent_at],
    )?;
    conn.execute(
        "UPDATE sync_outbox_input_buckets SET sent_at_utc = ?2 WHERE batch_uuid = ?1 AND sent_at_utc IS NULL",
        params![batch_uuid, sent_at],
    )?;
    conn.execute(
        "UPDATE sync_outbox_focus_buckets SET sent_at_utc = ?2 WHERE batch_uuid = ?1 AND sent_at_utc IS NULL",
        params![batch_uuid, sent_at],
    )?;
    Ok(())
}

pub fn seed_outbox_for_owned_rows(conn: &Connection, own_source_uuid: &str) -> Result<()> {
    let Some(source) = get_source_by_uuid(conn, own_source_uuid)? else {
        return Ok(());
    };

    enqueue_source_change(
        conn,
        &SourceChange {
            source_uuid: source.source_uuid.clone(),
            source_name: source.source_name.clone(),
            platform: source.platform.clone(),
            created_at_utc: Utc::now().to_rfc3339(),
        },
    )?;

    let mut input_stmt = conn.prepare(
        "
        SELECT bucket_start_utc, bucket_end_utc, local_date, local_hour, timezone_offset_minutes,
               granularity_minutes, left_clicks, right_clicks, middle_clicks, key_presses,
               mouse_distance_cm, scroll_vertical_cm, scroll_horizontal_cm
        FROM input_buckets
        WHERE source_id = ?1
        ",
    )?;
    let input_rows = input_stmt.query_map([source.id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, u32>(3)?,
            row.get::<_, i32>(4)?,
            row.get::<_, u32>(5)?,
            row.get::<_, u64>(6)?,
            row.get::<_, u64>(7)?,
            row.get::<_, u64>(8)?,
            row.get::<_, u64>(9)?,
            row.get::<_, f64>(10)?,
            row.get::<_, f64>(11)?,
            row.get::<_, f64>(12)?,
        ))
    })?;
    for row in input_rows {
        let row = row?;
        enqueue_input_change(
            conn,
            &InputBucketChange {
                source_uuid: own_source_uuid.to_string(),
                bucket_start_utc: row.0,
                bucket_end_utc: row.1,
                local_date: row.2,
                local_hour: row.3,
                timezone_offset_minutes: row.4,
                granularity_minutes: row.5,
                left_clicks: row.6,
                right_clicks: row.7,
                middle_clicks: row.8,
                key_presses: row.9,
                mouse_distance_cm: row.10,
                scroll_vertical_cm: row.11,
                scroll_horizontal_cm: row.12,
            },
        )?;
    }

    let mut focus_stmt = conn.prepare(
        "
        SELECT bucket_start_utc, bucket_end_utc, local_date, local_hour, timezone_offset_minutes,
               app_identifier, window_title, window_class, focus_seconds
        FROM focus_buckets
        WHERE source_id = ?1
        ",
    )?;
    let focus_rows = focus_stmt.query_map([source.id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, u32>(3)?,
            row.get::<_, i32>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, u64>(8)?,
        ))
    })?;
    for row in focus_rows {
        let row = row?;
        enqueue_focus_change(
            conn,
            &FocusBucketChange {
                source_uuid: own_source_uuid.to_string(),
                bucket_start_utc: row.0,
                bucket_end_utc: row.1,
                local_date: row.2,
                local_hour: row.3,
                timezone_offset_minutes: row.4,
                app_identifier: row.5,
                window_title: row.6,
                window_class: row.7,
                focus_seconds: row.8,
            },
        )?;
    }

    Ok(())
}

pub fn input_change_from_row(row: &InputBucketRecord, source_uuid: &str) -> InputBucketChange {
    InputBucketChange {
        source_uuid: source_uuid.to_string(),
        bucket_start_utc: row.bucket_start_utc.to_rfc3339(),
        bucket_end_utc: row.bucket_end_utc.to_rfc3339(),
        local_date: row.local_date.clone(),
        local_hour: row.local_hour,
        timezone_offset_minutes: row.timezone_offset_minutes,
        granularity_minutes: row.granularity_minutes,
        left_clicks: row.left_clicks,
        right_clicks: row.right_clicks,
        middle_clicks: row.middle_clicks,
        key_presses: row.key_presses,
        mouse_distance_cm: row.mouse_distance_cm,
        scroll_vertical_cm: row.scroll_vertical_cm,
        scroll_horizontal_cm: row.scroll_horizontal_cm,
    }
}

pub fn focus_change_from_row(row: &FocusBucketRecord, source_uuid: &str) -> FocusBucketChange {
    FocusBucketChange {
        source_uuid: source_uuid.to_string(),
        bucket_start_utc: row.bucket_start_utc.to_rfc3339(),
        bucket_end_utc: row.bucket_end_utc.to_rfc3339(),
        local_date: row.local_date.clone(),
        local_hour: row.local_hour,
        timezone_offset_minutes: row.timezone_offset_minutes,
        app_identifier: row.app_identifier.clone(),
        window_title: row.window_title.clone(),
        window_class: row.window_class.clone(),
        focus_seconds: row.focus_seconds,
    }
}

pub fn input_row_from_change(
    change: &InputBucketChange,
    source_id: i64,
) -> Result<InputBucketRecord> {
    Ok(InputBucketRecord {
        source_id,
        bucket_start_utc: parse_utc(change.bucket_start_utc.clone())?,
        bucket_end_utc: parse_utc(change.bucket_end_utc.clone())?,
        local_date: change.local_date.clone(),
        local_hour: change.local_hour,
        timezone_offset_minutes: change.timezone_offset_minutes,
        granularity_minutes: change.granularity_minutes,
        left_clicks: change.left_clicks,
        right_clicks: change.right_clicks,
        middle_clicks: change.middle_clicks,
        key_presses: change.key_presses,
        mouse_distance_cm: change.mouse_distance_cm,
        scroll_vertical_cm: change.scroll_vertical_cm,
        scroll_horizontal_cm: change.scroll_horizontal_cm,
    })
}

pub fn focus_row_from_change(
    change: &FocusBucketChange,
    source_id: i64,
) -> Result<FocusBucketRecord> {
    Ok(FocusBucketRecord {
        source_id,
        bucket_start_utc: parse_utc(change.bucket_start_utc.clone())?,
        bucket_end_utc: parse_utc(change.bucket_end_utc.clone())?,
        local_date: change.local_date.clone(),
        local_hour: change.local_hour,
        timezone_offset_minutes: change.timezone_offset_minutes,
        app_identifier: change.app_identifier.clone(),
        window_title: change.window_title.clone(),
        window_class: change.window_class.clone(),
        focus_seconds: change.focus_seconds,
    })
}

fn enqueue_source_change(conn: &Connection, change: &SourceChange) -> Result<()> {
    conn.execute(
        "
        INSERT INTO sync_outbox_sources (
            source_uuid, source_name, platform, created_at_utc
        ) VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(source_uuid) WHERE sent_at_utc IS NULL
        DO UPDATE SET
            source_name = excluded.source_name,
            platform = excluded.platform,
            created_at_utc = excluded.created_at_utc
        ",
        params![
            change.source_uuid,
            change.source_name,
            change.platform,
            change.created_at_utc
        ],
    )?;
    Ok(())
}

fn enqueue_input_change(conn: &Connection, change: &InputBucketChange) -> Result<()> {
    conn.execute(
        "
        INSERT INTO sync_outbox_input_buckets (
            source_uuid, bucket_start_utc, bucket_end_utc, local_date, local_hour,
            timezone_offset_minutes, granularity_minutes, left_clicks, right_clicks,
            middle_clicks, key_presses, mouse_distance_cm, scroll_vertical_cm,
            scroll_horizontal_cm, created_at_utc
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ON CONFLICT(source_uuid, bucket_start_utc, granularity_minutes) WHERE sent_at_utc IS NULL
        DO UPDATE SET
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
            scroll_horizontal_cm = excluded.scroll_horizontal_cm,
            created_at_utc = excluded.created_at_utc
        ",
        params![
            change.source_uuid,
            change.bucket_start_utc,
            change.bucket_end_utc,
            change.local_date,
            change.local_hour,
            change.timezone_offset_minutes,
            change.granularity_minutes,
            change.left_clicks,
            change.right_clicks,
            change.middle_clicks,
            change.key_presses,
            change.mouse_distance_cm,
            change.scroll_vertical_cm,
            change.scroll_horizontal_cm,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

fn enqueue_focus_change(conn: &Connection, change: &FocusBucketChange) -> Result<()> {
    conn.execute(
        "
        INSERT INTO sync_outbox_focus_buckets (
            source_uuid, bucket_start_utc, bucket_end_utc, local_date, local_hour,
            timezone_offset_minutes, app_identifier, window_title, window_class,
            focus_seconds, created_at_utc
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(source_uuid, bucket_start_utc, window_title, window_class) WHERE sent_at_utc IS NULL
        DO UPDATE SET
            bucket_end_utc = excluded.bucket_end_utc,
            local_date = excluded.local_date,
            local_hour = excluded.local_hour,
            timezone_offset_minutes = excluded.timezone_offset_minutes,
            app_identifier = excluded.app_identifier,
            focus_seconds = excluded.focus_seconds,
            created_at_utc = excluded.created_at_utc
        ",
        params![
            change.source_uuid,
            change.bucket_start_utc,
            change.bucket_end_utc,
            change.local_date,
            change.local_hour,
            change.timezone_offset_minutes,
            change.app_identifier,
            change.window_title,
            change.window_class,
            change.focus_seconds,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

fn pending_outbox_count(conn: &Connection, own_source_uuid: &str) -> Result<i64> {
    Ok(
        count_pending_table(conn, "sync_outbox_sources", own_source_uuid)?
            + count_pending_table(conn, "sync_outbox_input_buckets", own_source_uuid)?
            + count_pending_table(conn, "sync_outbox_focus_buckets", own_source_uuid)?,
    )
}

fn count_pending_table(conn: &Connection, table: &str, own_source_uuid: &str) -> Result<i64> {
    let sql =
        format!("SELECT COUNT(*) FROM {table} WHERE sent_at_utc IS NULL AND source_uuid = ?1");
    Ok(conn.query_row(&sql, [own_source_uuid], |row| row.get(0))?)
}

fn assign_batch_uuid(conn: &Connection, own_source_uuid: &str, batch_uuid: &str) -> Result<()> {
    for table in [
        "sync_outbox_sources",
        "sync_outbox_input_buckets",
        "sync_outbox_focus_buckets",
    ] {
        let sql = format!(
            "UPDATE {table} SET batch_uuid = ?2, attempt_count = attempt_count + 1 \
             WHERE sent_at_utc IS NULL AND source_uuid = ?1 AND batch_uuid IS NULL"
        );
        conn.execute(&sql, params![own_source_uuid, batch_uuid])?;
    }
    Ok(())
}

fn increment_batch_attempts(
    conn: &Connection,
    own_source_uuid: &str,
    batch_uuid: &str,
) -> Result<()> {
    for table in [
        "sync_outbox_sources",
        "sync_outbox_input_buckets",
        "sync_outbox_focus_buckets",
    ] {
        let sql = format!(
            "UPDATE {table} SET attempt_count = attempt_count + 1 \
             WHERE sent_at_utc IS NULL AND source_uuid = ?1 AND batch_uuid = ?2"
        );
        conn.execute(&sql, params![own_source_uuid, batch_uuid])?;
    }
    Ok(())
}

fn resolve_remote_source_id(
    conn: &Connection,
    source_uuid: &str,
    created_at_utc: &str,
) -> Result<i64> {
    let Some(source) = get_source_by_uuid(conn, source_uuid)? else {
        anyhow::bail!(
            "missing source metadata for remote source {} while applying bucket change at {}",
            source_uuid,
            created_at_utc
        );
    };
    Ok(source.id)
}

fn parse_utc(value: String) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(&value)
        .with_context(|| format!("invalid RFC3339 timestamp: {value}"))?
        .with_timezone(&Utc))
}
