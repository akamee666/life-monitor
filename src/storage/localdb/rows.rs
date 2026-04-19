use anyhow::{Context, Result};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::common::{FocusBucketRecord, InputBucketRecord, SourceInfo};

pub fn insert_input_buckets(conn: &Connection, rows: &[InputBucketRecord]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut stmt = conn.prepare_cached(
        "
        INSERT INTO input_buckets (
            source_id,
            bucket_start_utc,
            bucket_end_utc,
            local_date,
            local_hour,
            timezone_offset_minutes,
            granularity_minutes,
            left_clicks,
            right_clicks,
            middle_clicks,
            key_presses,
            mouse_distance_cm,
            scroll_vertical_cm,
            scroll_horizontal_cm
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(source_id, bucket_start_utc, granularity_minutes) DO UPDATE SET
            bucket_end_utc = excluded.bucket_end_utc,
            local_date = excluded.local_date,
            local_hour = excluded.local_hour,
            timezone_offset_minutes = excluded.timezone_offset_minutes,
            left_clicks = input_buckets.left_clicks + excluded.left_clicks,
            right_clicks = input_buckets.right_clicks + excluded.right_clicks,
            middle_clicks = input_buckets.middle_clicks + excluded.middle_clicks,
            key_presses = input_buckets.key_presses + excluded.key_presses,
            mouse_distance_cm = input_buckets.mouse_distance_cm + excluded.mouse_distance_cm,
            scroll_vertical_cm = input_buckets.scroll_vertical_cm + excluded.scroll_vertical_cm,
            scroll_horizontal_cm = input_buckets.scroll_horizontal_cm + excluded.scroll_horizontal_cm
        ",
    )?;

    for row in rows {
        stmt.execute(params![
            row.source_id,
            row.bucket_start_utc.to_rfc3339(),
            row.bucket_end_utc.to_rfc3339(),
            row.local_date,
            row.local_hour,
            row.timezone_offset_minutes,
            row.granularity_minutes,
            row.left_clicks,
            row.right_clicks,
            row.middle_clicks,
            row.key_presses,
            row.mouse_distance_cm,
            row.scroll_vertical_cm,
            row.scroll_horizontal_cm,
        ])
        .with_context(|| "Failed to insert input bucket row")?;
    }

    Ok(())
}

pub fn insert_focus_buckets(conn: &Connection, rows: &[FocusBucketRecord]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut stmt = conn.prepare_cached(
        "
        INSERT INTO focus_buckets (
            source_id,
            bucket_start_utc,
            bucket_end_utc,
            local_date,
            local_hour,
            timezone_offset_minutes,
            app_identifier,
            window_title,
            window_class,
            focus_seconds
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(source_id, bucket_start_utc, window_title, window_class) DO UPDATE SET
            bucket_end_utc = excluded.bucket_end_utc,
            local_date = excluded.local_date,
            local_hour = excluded.local_hour,
            timezone_offset_minutes = excluded.timezone_offset_minutes,
            app_identifier = excluded.app_identifier,
            focus_seconds = focus_buckets.focus_seconds + excluded.focus_seconds
        ",
    )?;

    for row in rows {
        stmt.execute(params![
            row.source_id,
            row.bucket_start_utc.to_rfc3339(),
            row.bucket_end_utc.to_rfc3339(),
            row.local_date,
            row.local_hour,
            row.timezone_offset_minutes,
            row.app_identifier,
            row.window_title,
            row.window_class,
            row.focus_seconds,
        ])
        .with_context(|| "Failed to insert focus bucket row")?;
    }

    Ok(())
}

pub fn get_source(conn: &Connection, source_id: i64) -> Result<SourceInfo> {
    conn.query_row(
        "SELECT id, source_uuid, source_name, platform FROM sources WHERE id = ?1",
        [source_id],
        |row| {
            Ok(SourceInfo {
                id: row.get(0)?,
                source_uuid: row.get(1)?,
                source_name: row.get(2)?,
                platform: row.get(3)?,
            })
        },
    )
    .with_context(|| format!("Failed to retrieve source row with id {source_id}"))
}

#[cfg_attr(not(feature = "multi-sync"), allow(dead_code))]
pub fn get_source_by_uuid(conn: &Connection, source_uuid: &str) -> Result<Option<SourceInfo>> {
    conn.query_row(
        "SELECT id, source_uuid, source_name, platform FROM sources WHERE source_uuid = ?1",
        [source_uuid],
        |row| {
            Ok(SourceInfo {
                id: row.get(0)?,
                source_uuid: row.get(1)?,
                source_name: row.get(2)?,
                platform: row.get(3)?,
            })
        },
    )
    .optional()
    .with_context(|| format!("Failed to retrieve source row with source_uuid {source_uuid}"))
}

#[cfg_attr(not(feature = "multi-sync"), allow(dead_code))]
pub fn upsert_source_by_uuid(
    conn: &Connection,
    source_uuid: &str,
    source_name: &str,
    platform: &str,
    created_at_utc: &str,
) -> Result<i64> {
    conn.execute(
        "
        INSERT INTO sources (source_uuid, source_name, platform, created_at_utc)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(source_uuid) DO UPDATE SET
            source_name = excluded.source_name,
            platform = excluded.platform
        ",
        params![source_uuid, source_name, platform, created_at_utc],
    )
    .with_context(|| format!("Failed to upsert source row for source_uuid {source_uuid}"))?;

    Ok(conn.query_row(
        "SELECT id FROM sources WHERE source_uuid = ?1",
        [source_uuid],
        |row| row.get(0),
    )?)
}

pub fn open_con_at(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for sqlite database: {}",
                parent.display()
            )
        })?;
    }

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .with_context(|| {
        format!(
            "Failed to open database connection with sqlite database at: {}",
            db_path.display()
        )
    })?;
    conn.busy_timeout(Duration::from_secs(5))
        .with_context(|| "Failed to configure sqlite busy timeout")?;
    Ok(conn)
}
