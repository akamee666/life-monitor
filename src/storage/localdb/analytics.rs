use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub session_uuid: String,
    pub source_uuid: String,
    pub source_name: String,
    pub started_at_utc: String,
    pub ended_at_utc: Option<String>,
    pub platform: String,
    pub duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppActivityRow {
    pub source_uuid: String,
    pub source_name: String,
    pub platform: String,
    pub app_identifier: String,
    pub focus_seconds: u64,
}

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

#[derive(Debug, Clone, PartialEq)]
pub struct SessionStatsRow {
    pub source_uuid: String,
    pub source_name: String,
    pub platform: String,
    pub session_count: u64,
    pub open_session_count: u64,
    pub total_duration_seconds: u64,
    pub average_duration_seconds: u64,
    pub longest_duration_seconds: u64,
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

#[allow(dead_code)]
pub fn current_open_session(conn: &Connection, source_id: i64) -> Result<Option<SessionRow>> {
    conn.query_row(
        "
        SELECT sess.session_uuid, src.source_uuid, src.source_name, sess.started_at_utc, sess.ended_at_utc, sess.platform
        FROM sessions sess
        JOIN sources src ON src.id = sess.source_id
        WHERE sess.source_id = ?1 AND sess.ended_at_utc IS NULL
        ORDER BY sess.started_at_utc DESC
        LIMIT 1
        ",
        [source_id],
        |row| {
            Ok(SessionRow {
                session_uuid: row.get(0)?,
                source_uuid: row.get(1)?,
                source_name: row.get(2)?,
                started_at_utc: row.get(3)?,
                ended_at_utc: row.get(4)?,
                platform: row.get(5)?,
                duration_seconds: None,
            })
        },
    )
    .optional()
    .with_context(|| "Failed to query the currently open session")
}

pub fn session_report(conn: &Connection, days: u32) -> Result<Vec<SessionRow>> {
    let since = Utc::now() - Duration::days(days.max(1) as i64);
    let mut stmt = conn.prepare(
        "
        SELECT sess.session_uuid, src.source_uuid, src.source_name, sess.started_at_utc, sess.ended_at_utc, sess.platform
        FROM sessions sess
        JOIN sources src ON src.id = sess.source_id
        WHERE sess.started_at_utc >= ?1
        ORDER BY sess.started_at_utc DESC, src.source_name ASC
        ",
    )?;
    let rows = stmt.query_map([since.to_rfc3339()], |row| {
        let started_at_utc: String = row.get(3)?;
        let ended_at_utc: Option<String> = row.get(4)?;
        let duration_seconds = compute_duration_seconds(&started_at_utc, ended_at_utc.as_deref());

        Ok(SessionRow {
            session_uuid: row.get(0)?,
            source_uuid: row.get(1)?,
            source_name: row.get(2)?,
            started_at_utc,
            ended_at_utc,
            platform: row.get(5)?,
            duration_seconds,
        })
    })?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .with_context(|| "Failed to load session analytics rows")
}

pub fn app_activity_report(conn: &Connection, days: u32) -> Result<Vec<AppActivityRow>> {
    let since = Utc::now() - Duration::days(days.max(1) as i64);
    let mut stmt = conn.prepare(
        "
        SELECT src.source_uuid, src.source_name, src.platform, focus.app_identifier,
               SUM(focus.focus_seconds) AS total_focus_seconds
        FROM focus_buckets focus
        JOIN sources src ON src.id = focus.source_id
        WHERE focus.bucket_start_utc >= ?1
        GROUP BY src.source_uuid, src.source_name, src.platform, focus.app_identifier
        ORDER BY total_focus_seconds DESC, src.source_name ASC, focus.app_identifier ASC
        ",
    )?;

    let rows = stmt.query_map([since.to_rfc3339()], |row| {
        Ok(AppActivityRow {
            source_uuid: row.get(0)?,
            source_name: row.get(1)?,
            platform: row.get(2)?,
            app_identifier: row.get(3)?,
            focus_seconds: row.get::<_, Option<u64>>(4)?.unwrap_or(0),
        })
    })?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .with_context(|| "Failed to load app activity analytics rows")
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

pub fn session_stats_report(conn: &Connection, days: u32) -> Result<Vec<SessionStatsRow>> {
    let mut stats = BTreeMap::<String, SessionStatsRow>::new();
    for row in session_report(conn, days)? {
        let entry = stats
            .entry(row.source_uuid.clone())
            .or_insert(SessionStatsRow {
                source_uuid: row.source_uuid.clone(),
                source_name: row.source_name.clone(),
                platform: row.platform.clone(),
                session_count: 0,
                open_session_count: 0,
                total_duration_seconds: 0,
                average_duration_seconds: 0,
                longest_duration_seconds: 0,
            });
        entry.session_count += 1;
        if row.ended_at_utc.is_none() {
            entry.open_session_count += 1;
        }
        if let Some(duration) = row.duration_seconds {
            entry.total_duration_seconds += duration;
            entry.longest_duration_seconds = entry.longest_duration_seconds.max(duration);
        }
    }

    let mut rows = stats.into_values().collect::<Vec<_>>();
    for row in &mut rows {
        if row.session_count > 0 {
            row.average_duration_seconds = row.total_duration_seconds / row.session_count;
        }
    }
    rows.sort_by(|left, right| {
        right
            .total_duration_seconds
            .cmp(&left.total_duration_seconds)
            .then_with(|| left.source_name.cmp(&right.source_name))
    });
    Ok(rows)
}

fn compute_duration_seconds(started_at_utc: &str, ended_at_utc: Option<&str>) -> Option<u64> {
    let started_at = DateTime::parse_from_rfc3339(started_at_utc).ok()?;
    let ended_at = ended_at_utc
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .unwrap_or_else(|| Utc::now().fixed_offset());

    Some((ended_at - started_at).num_seconds().max(0) as u64)
}
