use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct DailyActivityRow {
    pub local_date: String,
    pub source_uuid: String,
    pub source_name: String,
    pub platform: String,
    pub key_presses: u64,
    pub left_clicks: u64,
    pub right_clicks: u64,
    pub middle_clicks: u64,
    pub mouse_distance_cm: f64,
    pub scroll_vertical_cm: f64,
    pub scroll_horizontal_cm: f64,
    pub focus_seconds: u64,
}

pub fn begin_session(conn: &Connection, source_id: i64, platform: &str) -> Result<String> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "
        UPDATE sessions
        SET ended_at_utc = ?1
        WHERE source_id = ?2 AND ended_at_utc IS NULL
        ",
        params![now, source_id],
    )
    .with_context(|| "Failed to close previously open sessions")?;

    let session_uuid = Uuid::new_v4().to_string();
    conn.execute(
        "
        INSERT INTO sessions (source_id, started_at_utc, ended_at_utc, session_uuid, platform)
        VALUES (?1, ?2, NULL, ?3, ?4)
        ",
        params![source_id, now, session_uuid, platform],
    )
    .with_context(|| "Failed to record the current collection session")?;

    Ok(session_uuid)
}

pub fn end_session(conn: &Connection, session_uuid: &str) -> Result<()> {
    conn.execute(
        "
        UPDATE sessions
        SET ended_at_utc = COALESCE(ended_at_utc, ?1)
        WHERE session_uuid = ?2
        ",
        params![Utc::now().to_rfc3339(), session_uuid],
    )
    .with_context(|| format!("Failed to finalize session {session_uuid}"))?;
    Ok(())
}

pub fn daily_activity_report(conn: &Connection, days: u32) -> Result<Vec<DailyActivityRow>> {
    let since = Utc::now() - Duration::days(days.max(1) as i64);
    let mut rows = BTreeMap::<(String, String), DailyActivityRow>::new();

    // Merge input and focus aggregates in Rust by (local_date, source_uuid). That keeps the
    // reporting logic explicit without introducing extra persisted summary tables.
    let mut input_stmt = conn.prepare(
        "
        SELECT input.local_date, src.source_uuid, src.source_name, src.platform,
               SUM(input.key_presses), SUM(input.left_clicks), SUM(input.right_clicks),
               SUM(input.middle_clicks), SUM(input.mouse_distance_cm),
               SUM(input.scroll_vertical_cm), SUM(input.scroll_horizontal_cm)
        FROM input_buckets input
        JOIN sources src ON src.id = input.source_id
        WHERE input.bucket_start_utc >= ?1
        GROUP BY input.local_date, src.source_uuid, src.source_name, src.platform
        ",
    )?;
    let input_rows = input_stmt.query_map([since.to_rfc3339()], |row| {
        Ok(DailyActivityRow {
            local_date: row.get(0)?,
            source_uuid: row.get(1)?,
            source_name: row.get(2)?,
            platform: row.get(3)?,
            key_presses: row.get::<_, Option<u64>>(4)?.unwrap_or(0),
            left_clicks: row.get::<_, Option<u64>>(5)?.unwrap_or(0),
            right_clicks: row.get::<_, Option<u64>>(6)?.unwrap_or(0),
            middle_clicks: row.get::<_, Option<u64>>(7)?.unwrap_or(0),
            mouse_distance_cm: row.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
            scroll_vertical_cm: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
            scroll_horizontal_cm: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
            focus_seconds: 0,
        })
    })?;
    for row in input_rows {
        let row = row?;
        rows.insert((row.local_date.clone(), row.source_uuid.clone()), row);
    }

    let mut focus_stmt = conn.prepare(
        "
        SELECT focus.local_date, src.source_uuid, src.source_name, src.platform,
               SUM(focus.focus_seconds)
        FROM focus_buckets focus
        JOIN sources src ON src.id = focus.source_id
        WHERE focus.bucket_start_utc >= ?1
        GROUP BY focus.local_date, src.source_uuid, src.source_name, src.platform
        ",
    )?;
    let focus_rows = focus_stmt.query_map([since.to_rfc3339()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<u64>>(4)?.unwrap_or(0),
        ))
    })?;
    for row in focus_rows {
        let (local_date, source_uuid, source_name, platform, focus_seconds) = row?;
        let entry = rows
            .entry((local_date.clone(), source_uuid.clone()))
            .or_insert(DailyActivityRow {
                local_date,
                source_uuid,
                source_name,
                platform,
                key_presses: 0,
                left_clicks: 0,
                right_clicks: 0,
                middle_clicks: 0,
                mouse_distance_cm: 0.0,
                scroll_vertical_cm: 0.0,
                scroll_horizontal_cm: 0.0,
                focus_seconds: 0,
            });
        entry.focus_seconds = focus_seconds;
    }

    let mut rows = rows.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .local_date
            .cmp(&left.local_date)
            .then_with(|| left.source_name.cmp(&right.source_name))
    });
    Ok(rows)
}
