use anyhow::{bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::path::{Path, PathBuf};
use tracing::info;
use uuid::Uuid;

use crate::utils::lock::acquire_db_operation_lock;

use super::export::{latest_export_metadata, latest_export_metadata_from_attached, ExportMetadata};
use super::integrity::{
    attach_source, backup_database_contents, default_pre_import_backup_path, detach_source,
    file_sha256, run_integrity_check, scalar_query_u64, validate_schema_version,
};
use super::rows::open_con_at;
use super::schema::setup_database;

#[derive(Debug, Clone, PartialEq)]
pub struct ImportPlan {
    pub source_export_uuid: String,
    pub source_file_hash: String,
    pub duplicate_import: bool,
    pub duplicate_reason: Option<String>,
    pub new_sources: u64,
    pub new_input_buckets: u64,
    pub updated_input_buckets: u64,
    pub input_key_presses_delta: u64,
    pub input_left_clicks_delta: u64,
    pub input_right_clicks_delta: u64,
    pub input_middle_clicks_delta: u64,
    pub input_mouse_distance_cm_delta: f64,
    pub new_focus_buckets: u64,
    pub updated_focus_buckets: u64,
    pub focus_seconds_delta: u64,
}

impl ImportPlan {
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("import snapshot {}", self.source_export_uuid));

        if self.duplicate_import {
            lines.push(format!(
                "! duplicate import detected: {}",
                self.duplicate_reason
                    .as_deref()
                    .unwrap_or("this snapshot was already imported")
            ));
        }

        lines.push(format!("+ sources: {} new", self.new_sources));
        lines.push(format!("+ input buckets: {} new", self.new_input_buckets));
        lines.push(format!(
            "~ input buckets: {} existing rows will be incremented",
            self.updated_input_buckets
        ));
        lines.push(format!(
            "~ input totals: key_presses +{}, left_clicks +{}, right_clicks +{}, middle_clicks +{}, mouse_distance_cm +{:.2}",
            self.input_key_presses_delta,
            self.input_left_clicks_delta,
            self.input_right_clicks_delta,
            self.input_middle_clicks_delta,
            self.input_mouse_distance_cm_delta
        ));
        lines.push(format!("+ focus buckets: {} new", self.new_focus_buckets));
        lines.push(format!(
            "~ focus buckets: {} existing rows will be incremented",
            self.updated_focus_buckets
        ));
        lines.push(format!(
            "~ focus totals: focus_seconds +{}",
            self.focus_seconds_delta
        ));
        lines.join("\n")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportResult {
    pub destination_backup_path: PathBuf,
    pub plan: ImportPlan,
}

pub fn plan_import(destination_db_path: &Path, source_snapshot_path: &Path) -> Result<ImportPlan> {
    let _lock = acquire_db_operation_lock(destination_db_path)?;
    info!(
        "Planning snapshot import from '{}' into '{}'.",
        source_snapshot_path.display(),
        destination_db_path.display()
    );
    plan_import_locked(destination_db_path, source_snapshot_path)
}

fn plan_import_locked(
    destination_db_path: &Path,
    source_snapshot_path: &Path,
) -> Result<ImportPlan> {
    let destination = open_con_at(destination_db_path)?;
    setup_database(&destination)?;
    let source =
        Connection::open_with_flags(source_snapshot_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| {
                format!(
                    "Failed to open source snapshot for import planning: {}",
                    source_snapshot_path.display()
                )
            })?;

    run_integrity_check(&destination, "destination database")?;
    run_integrity_check(&source, "source snapshot")?;
    validate_schema_version(&destination, "destination database")?;
    validate_schema_version(&source, "source snapshot")?;

    let metadata = latest_export_metadata(&source)?;
    let file_hash = file_sha256(source_snapshot_path)?;
    let duplicate_reason = existing_import_reason(&destination, &metadata.export_uuid, &file_hash)?;

    attach_source(&destination, source_snapshot_path)?;
    let plan = query_import_plan(&destination, &metadata, &file_hash, duplicate_reason)?;
    detach_source(&destination)?;
    Ok(plan)
}

pub fn import_snapshot(
    destination_db_path: &Path,
    source_snapshot_path: &Path,
    notes: Option<&str>,
) -> Result<ImportResult> {
    let _lock = acquire_db_operation_lock(destination_db_path)?;
    info!(
        "Starting snapshot import from '{}' into '{}'.",
        source_snapshot_path.display(),
        destination_db_path.display()
    );
    let plan = plan_import_locked(destination_db_path, source_snapshot_path)?;
    if plan.duplicate_import {
        bail!(
            "Import refused because this snapshot was already imported: {}",
            plan.duplicate_reason
                .as_deref()
                .unwrap_or("duplicate snapshot")
        );
    }

    let backup_path = default_pre_import_backup_path(destination_db_path);
    info!(
        "Creating automatic pre-import backup at '{}'.",
        backup_path.display()
    );
    let destination_for_backup = open_con_at(destination_db_path)?;
    backup_database_contents(&destination_for_backup, &backup_path)
        .with_context(|| "Failed to create automatic destination backup before import")?;

    let destination = open_con_at(destination_db_path)?;
    attach_source(&destination, source_snapshot_path)?;
    let metadata = latest_export_metadata_from_attached(&destination)?;

    let tx = destination.unchecked_transaction()?;
    tx.execute_batch(
        "
        UPDATE sources
        SET
            source_name = src.source_name,
            platform    = src.platform
        FROM import_src.sources src
        WHERE src.source_uuid = sources.source_uuid;

        INSERT OR IGNORE INTO sources (source_uuid, source_name, platform, created_at_utc)
        SELECT source_uuid, source_name, platform, created_at_utc
        FROM import_src.sources;

        UPDATE input_buckets
        SET
            bucket_end_utc          = ib.bucket_end_utc,
            local_date              = ib.local_date,
            local_hour              = ib.local_hour,
            timezone_offset_minutes = ib.timezone_offset_minutes,
            left_clicks             = input_buckets.left_clicks          + ib.left_clicks,
            right_clicks            = input_buckets.right_clicks         + ib.right_clicks,
            middle_clicks           = input_buckets.middle_clicks        + ib.middle_clicks,
            key_presses             = input_buckets.key_presses          + ib.key_presses,
            mouse_distance_cm       = input_buckets.mouse_distance_cm    + ib.mouse_distance_cm,
            scroll_vertical_cm      = input_buckets.scroll_vertical_cm   + ib.scroll_vertical_cm,
            scroll_horizontal_cm    = input_buckets.scroll_horizontal_cm + ib.scroll_horizontal_cm
        FROM import_src.input_buckets ib
        JOIN import_src.sources src_src ON src_src.id = ib.source_id
        JOIN sources dest_src ON dest_src.source_uuid = src_src.source_uuid
        WHERE input_buckets.source_id           = dest_src.id
          AND input_buckets.bucket_start_utc    = ib.bucket_start_utc
          AND input_buckets.granularity_minutes = ib.granularity_minutes;

        INSERT INTO input_buckets (
            source_id, bucket_start_utc, bucket_end_utc,
            local_date, local_hour, timezone_offset_minutes, granularity_minutes,
            left_clicks, right_clicks, middle_clicks, key_presses,
            mouse_distance_cm, scroll_vertical_cm, scroll_horizontal_cm
        )
        SELECT
            dest_src.id, ib.bucket_start_utc, ib.bucket_end_utc,
            ib.local_date, ib.local_hour, ib.timezone_offset_minutes, ib.granularity_minutes,
            ib.left_clicks, ib.right_clicks, ib.middle_clicks, ib.key_presses,
            ib.mouse_distance_cm, ib.scroll_vertical_cm, ib.scroll_horizontal_cm
        FROM import_src.input_buckets ib
        JOIN import_src.sources src_src ON src_src.id = ib.source_id
        JOIN sources dest_src ON dest_src.source_uuid = src_src.source_uuid
        WHERE NOT EXISTS (
            SELECT 1 FROM input_buckets dest
            WHERE dest.source_id           = dest_src.id
              AND dest.bucket_start_utc    = ib.bucket_start_utc
              AND dest.granularity_minutes = ib.granularity_minutes
        );

        UPDATE focus_buckets
        SET
            bucket_end_utc          = fb.bucket_end_utc,
            local_date              = fb.local_date,
            local_hour              = fb.local_hour,
            timezone_offset_minutes = fb.timezone_offset_minutes,
            app_identifier          = fb.app_identifier,
            focus_seconds           = focus_buckets.focus_seconds + fb.focus_seconds
        FROM import_src.focus_buckets fb
        JOIN import_src.sources src_src ON src_src.id = fb.source_id
        JOIN sources dest_src ON dest_src.source_uuid = src_src.source_uuid
        WHERE focus_buckets.source_id        = dest_src.id
          AND focus_buckets.bucket_start_utc = fb.bucket_start_utc
          AND focus_buckets.window_title     = fb.window_title
          AND focus_buckets.window_class     = fb.window_class;

        INSERT INTO focus_buckets (
            source_id, bucket_start_utc, bucket_end_utc,
            local_date, local_hour, timezone_offset_minutes,
            app_identifier, window_title, window_class, focus_seconds
        )
        SELECT
            dest_src.id, fb.bucket_start_utc, fb.bucket_end_utc,
            fb.local_date, fb.local_hour, fb.timezone_offset_minutes,
            fb.app_identifier, fb.window_title, fb.window_class, fb.focus_seconds
        FROM import_src.focus_buckets fb
        JOIN import_src.sources src_src ON src_src.id = fb.source_id
        JOIN sources dest_src ON dest_src.source_uuid = src_src.source_uuid
        WHERE NOT EXISTS (
            SELECT 1 FROM focus_buckets dest
            WHERE dest.source_id        = dest_src.id
              AND dest.bucket_start_utc = fb.bucket_start_utc
              AND dest.window_title     = fb.window_title
              AND dest.window_class     = fb.window_class
        );
        ",
    )?;

    tx.execute(
        "
        INSERT INTO imports (
            import_uuid,
            source_export_uuid,
            source_source_uuid,
            exported_at_utc,
            imported_at_utc,
            file_hash,
            schema_version,
            notes
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            Uuid::new_v4().to_string(),
            metadata.export_uuid,
            metadata.primary_source_uuid,
            metadata.exported_at_utc,
            Utc::now().to_rfc3339(),
            plan.source_file_hash,
            metadata.schema_version,
            notes,
        ],
    )?;
    tx.commit()?;

    detach_source(&destination)?;

    Ok(ImportResult {
        destination_backup_path: backup_path,
        plan,
    })
}

fn query_import_plan(
    conn: &Connection,
    metadata: &ExportMetadata,
    file_hash: &str,
    duplicate_reason: Option<String>,
) -> Result<ImportPlan> {
    let new_sources = scalar_query_u64(
        conn,
        "
        SELECT COUNT(*)
        FROM import_src.sources src
        LEFT JOIN main.sources dest ON dest.source_uuid = src.source_uuid
        WHERE dest.id IS NULL
        ",
    )?;

    let (
        new_input_buckets,
        updated_input_buckets,
        input_key_presses_delta,
        input_left_clicks_delta,
        input_right_clicks_delta,
        input_middle_clicks_delta,
        input_mouse_distance_cm_delta,
    ): (u64, u64, u64, u64, u64, u64, f64) = conn.query_row(
        "
            SELECT
                SUM(CASE WHEN existing.id IS NULL THEN 1 ELSE 0 END),
                SUM(CASE WHEN existing.id IS NOT NULL THEN 1 ELSE 0 END),
                COALESCE(SUM(ib.key_presses), 0),
                COALESCE(SUM(ib.left_clicks), 0),
                COALESCE(SUM(ib.right_clicks), 0),
                COALESCE(SUM(ib.middle_clicks), 0),
                COALESCE(SUM(ib.mouse_distance_cm), 0.0)
            FROM import_src.input_buckets ib
            JOIN import_src.sources src_src ON src_src.id = ib.source_id
            LEFT JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
            LEFT JOIN main.input_buckets existing
                ON existing.source_id = dest_src.id
               AND existing.bucket_start_utc = ib.bucket_start_utc
               AND existing.granularity_minutes = ib.granularity_minutes
            ",
        [],
        |row| {
            Ok((
                row.get::<_, Option<u64>>(0)?.unwrap_or(0),
                row.get::<_, Option<u64>>(1)?.unwrap_or(0),
                row.get::<_, Option<u64>>(2)?.unwrap_or(0),
                row.get::<_, Option<u64>>(3)?.unwrap_or(0),
                row.get::<_, Option<u64>>(4)?.unwrap_or(0),
                row.get::<_, Option<u64>>(5)?.unwrap_or(0),
                row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
            ))
        },
    )?;

    let (new_focus_buckets, updated_focus_buckets, focus_seconds_delta): (u64, u64, u64) = conn
        .query_row(
            "
            SELECT
                SUM(CASE WHEN existing.id IS NULL THEN 1 ELSE 0 END),
                SUM(CASE WHEN existing.id IS NOT NULL THEN 1 ELSE 0 END),
                COALESCE(SUM(fb.focus_seconds), 0)
            FROM import_src.focus_buckets fb
            JOIN import_src.sources src_src ON src_src.id = fb.source_id
            LEFT JOIN main.sources dest_src ON dest_src.source_uuid = src_src.source_uuid
            LEFT JOIN main.focus_buckets existing
                ON existing.source_id = dest_src.id
               AND existing.bucket_start_utc = fb.bucket_start_utc
               AND existing.window_title = fb.window_title
               AND existing.window_class = fb.window_class
            ",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<u64>>(0)?.unwrap_or(0),
                    row.get::<_, Option<u64>>(1)?.unwrap_or(0),
                    row.get::<_, Option<u64>>(2)?.unwrap_or(0),
                ))
            },
        )?;

    Ok(ImportPlan {
        source_export_uuid: metadata.export_uuid.clone(),
        source_file_hash: file_hash.to_string(),
        duplicate_import: duplicate_reason.is_some(),
        duplicate_reason,
        new_sources,
        new_input_buckets,
        updated_input_buckets,
        input_key_presses_delta,
        input_left_clicks_delta,
        input_right_clicks_delta,
        input_middle_clicks_delta,
        input_mouse_distance_cm_delta,
        new_focus_buckets,
        updated_focus_buckets,
        focus_seconds_delta,
    })
}

fn existing_import_reason(
    conn: &Connection,
    export_uuid: &str,
    file_hash: &str,
) -> Result<Option<String>> {
    let existing_export_uuid: Option<String> = conn
        .query_row(
            "SELECT source_export_uuid FROM imports WHERE source_export_uuid = ?1",
            [export_uuid],
            |row| row.get(0),
        )
        .optional()?;
    if existing_export_uuid.is_some() {
        return Ok(Some(format!(
            "snapshot export UUID '{}' was already imported",
            export_uuid
        )));
    }

    let existing_hash: Option<String> = conn
        .query_row(
            "SELECT file_hash FROM imports WHERE file_hash = ?1",
            [file_hash],
            |row| row.get(0),
        )
        .optional()?;
    if existing_hash.is_some() {
        return Ok(Some(
            "snapshot file hash already exists in imports history".to_string(),
        ));
    }

    Ok(None)
}
