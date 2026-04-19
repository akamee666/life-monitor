use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub session_uuid: String,
    pub started_at_utc: String,
    pub ended_at_utc: Option<String>,
    pub platform: String,
    pub duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppActivityRow {
    pub app_identifier: String,
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

#[allow(dead_code)]
pub fn current_open_session(conn: &Connection, source_id: i64) -> Result<Option<SessionRow>> {
    conn.query_row(
        "
        SELECT session_uuid, started_at_utc, ended_at_utc, platform
        FROM sessions
        WHERE source_id = ?1 AND ended_at_utc IS NULL
        ORDER BY started_at_utc DESC
        LIMIT 1
        ",
        [source_id],
        |row| {
            Ok(SessionRow {
                session_uuid: row.get(0)?,
                started_at_utc: row.get(1)?,
                ended_at_utc: row.get(2)?,
                platform: row.get(3)?,
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
        SELECT session_uuid, started_at_utc, ended_at_utc, platform
        FROM sessions
        WHERE started_at_utc >= ?1
        ORDER BY started_at_utc DESC
        ",
    )?;
    let rows = stmt.query_map([since.to_rfc3339()], |row| {
        let started_at_utc: String = row.get(1)?;
        let ended_at_utc: Option<String> = row.get(2)?;
        let duration_seconds = compute_duration_seconds(&started_at_utc, ended_at_utc.as_deref());

        Ok(SessionRow {
            session_uuid: row.get(0)?,
            started_at_utc,
            ended_at_utc,
            platform: row.get(3)?,
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
        SELECT app_identifier, SUM(focus_seconds) AS total_focus_seconds
        FROM focus_buckets
        WHERE bucket_start_utc >= ?1
        GROUP BY app_identifier
        ORDER BY total_focus_seconds DESC, app_identifier ASC
        ",
    )?;

    let rows = stmt.query_map([since.to_rfc3339()], |row| {
        Ok(AppActivityRow {
            app_identifier: row.get(0)?,
            focus_seconds: row.get::<_, Option<u64>>(1)?.unwrap_or(0),
        })
    })?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .with_context(|| "Failed to load app activity analytics rows")
}

fn compute_duration_seconds(started_at_utc: &str, ended_at_utc: Option<&str>) -> Option<u64> {
    let started_at = DateTime::parse_from_rfc3339(started_at_utc).ok()?;
    let ended_at = ended_at_utc
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .unwrap_or_else(|| Utc::now().fixed_offset());

    Some((ended_at - started_at).num_seconds().max(0) as u64)
}
