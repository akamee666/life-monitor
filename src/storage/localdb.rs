mod analytics;
mod config;
mod export;
mod import;
mod integrity;
mod rows;
mod schema;

#[allow(unused_imports)]
pub use analytics::{begin_session, daily_activity_report, end_session, DailyActivityRow};
#[allow(unused_imports)]
pub use config::{default_db_path, resolve_db_path, DbConfig, DbPathSource};
#[allow(unused_imports)]
pub use export::{export_database, ExportMetadata, ExportResult};
#[allow(unused_imports)]
pub use import::{import_snapshot, plan_import, ImportPlan, ImportResult};
#[allow(unused_imports)]
pub use rows::{
    get_source, get_source_by_uuid, insert_focus_buckets, insert_input_buckets, open_con_at,
    upsert_source_by_uuid,
};
#[allow(unused_imports)]
pub use schema::{clear_database, setup_database, SCHEMA_VERSION};

#[cfg(test)]
pub(crate) use export::latest_export_metadata;
#[cfg(test)]
pub(crate) use integrity::{file_sha256, scalar_query_u64};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{FocusBucketRecord, InputBucketRecord, DEFAULT_SOURCE_ID};
    use chrono::{Duration, TimeZone, Utc};
    use rusqlite::OptionalExtension;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use uuid::Uuid;

    fn unique_temp_db(name: &str) -> PathBuf {
        let suffix = Uuid::new_v4();
        std::env::temp_dir().join(format!("vigil-{name}-{suffix}.db"))
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn build_test_db(path: &Path) -> anyhow::Result<rusqlite::Connection> {
        let conn = open_con_at(path)?;
        setup_database(&conn)?;
        Ok(conn)
    }

    fn sample_input_row() -> InputBucketRecord {
        InputBucketRecord {
            source_id: DEFAULT_SOURCE_ID,
            bucket_start_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap(),
            bucket_end_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 15, 0).unwrap(),
            local_date: "2026-04-18".to_string(),
            local_hour: 9,
            timezone_offset_minutes: -180,
            granularity_minutes: 15,
            left_clicks: 2,
            right_clicks: 1,
            middle_clicks: 0,
            key_presses: 5,
            mouse_distance_cm: 3.0,
            scroll_vertical_cm: 0.4,
            scroll_horizontal_cm: 0.0,
        }
    }

    fn sample_focus_row() -> FocusBucketRecord {
        FocusBucketRecord {
            source_id: DEFAULT_SOURCE_ID,
            bucket_start_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 0).unwrap(),
            bucket_end_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 15, 0).unwrap(),
            local_date: "2026-04-18".to_string(),
            local_hour: 9,
            timezone_offset_minutes: -180,
            app_identifier: "firefox".to_string(),
            window_title: "Docs".to_string(),
            window_class: "firefox".to_string(),
            focus_seconds: 120,
        }
    }

    /// Verifies that DB path memory overwrites the previous remembered location by using an
    /// isolated data dir override and reading the config back through the public constructor.
    #[test]
    fn db_config_remembers_and_overwrites_last_db_path() -> anyhow::Result<()> {
        let _guard = env_lock().lock().unwrap();
        let data_dir = unique_temp_db("remembered-config-dir");
        std::fs::create_dir_all(&data_dir)?;
        std::env::set_var("VIGIL_DATA_DIR", &data_dir);

        let first_path = unique_temp_db("remembered-first");
        let second_path = unique_temp_db("remembered-second");

        let first = DbConfig::from_cli_path(Some(first_path.clone()))?;
        assert_eq!(first.db_path, first_path);
        assert_eq!(first.source, DbPathSource::Cli);

        let remembered = DbConfig::from_cli_path(None)?;
        assert_eq!(remembered.db_path, first_path);
        assert_eq!(remembered.source, DbPathSource::Remembered);

        let second = DbConfig::from_cli_path(Some(second_path.clone()))?;
        assert_eq!(second.db_path, second_path);

        let remembered_again = DbConfig::from_cli_path(None)?;
        assert_eq!(remembered_again.db_path, second_path);
        assert_eq!(remembered_again.source, DbPathSource::Remembered);

        std::env::remove_var("VIGIL_DATA_DIR");
        fs::remove_dir_all(data_dir)?;
        Ok(())
    }

    fn sample_second_input_row() -> InputBucketRecord {
        InputBucketRecord {
            bucket_start_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 15, 0).unwrap(),
            bucket_end_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 30, 0).unwrap(),
            local_hour: 9,
            key_presses: 7,
            left_clicks: 3,
            mouse_distance_cm: 1.5,
            ..sample_input_row()
        }
    }

    fn sample_second_focus_row() -> FocusBucketRecord {
        FocusBucketRecord {
            bucket_start_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 15, 0).unwrap(),
            bucket_end_utc: Utc.with_ymd_and_hms(2026, 4, 18, 12, 30, 0).unwrap(),
            local_hour: 9,
            window_title: "Mail".to_string(),
            focus_seconds: 45,
            ..sample_focus_row()
        }
    }

    fn report_test_input_rows() -> [InputBucketRecord; 2] {
        let report_day = (Utc::now() - Duration::days(1)).date_naive();
        let local_date = report_day.format("%F").to_string();
        let first_start = Utc.from_utc_datetime(&report_day.and_hms_opt(12, 0, 0).unwrap());
        let second_start = Utc.from_utc_datetime(&report_day.and_hms_opt(12, 15, 0).unwrap());

        [
            InputBucketRecord {
                bucket_start_utc: first_start,
                bucket_end_utc: first_start + Duration::minutes(15),
                local_date: local_date.clone(),
                ..sample_input_row()
            },
            InputBucketRecord {
                bucket_start_utc: second_start,
                bucket_end_utc: second_start + Duration::minutes(15),
                local_date,
                ..sample_second_input_row()
            },
        ]
    }

    fn report_test_focus_rows() -> [FocusBucketRecord; 2] {
        let report_day = (Utc::now() - Duration::days(1)).date_naive();
        let local_date = report_day.format("%F").to_string();
        let first_start = Utc.from_utc_datetime(&report_day.and_hms_opt(12, 0, 0).unwrap());
        let second_start = Utc.from_utc_datetime(&report_day.and_hms_opt(12, 15, 0).unwrap());

        [
            FocusBucketRecord {
                bucket_start_utc: first_start,
                bucket_end_utc: first_start + Duration::minutes(15),
                local_date: local_date.clone(),
                ..sample_focus_row()
            },
            FocusBucketRecord {
                bucket_start_utc: second_start,
                bucket_end_utc: second_start + Duration::minutes(15),
                local_date,
                ..sample_second_focus_row()
            },
        ]
    }

    /// Verifies that a direct custom file path is preserved as-is while ensuring the parent
    /// directory exists, which is the observable contract for explicit file destinations.
    #[test]
    fn resolve_db_path_uses_custom_location() -> anyhow::Result<()> {
        let path = unique_temp_db("custom-path").join("nested/data.db");
        let resolved = resolve_db_path(Some(&path))?;
        assert_eq!(resolved, path);
        assert!(resolved.parent().unwrap().exists());
        Ok(())
    }

    /// Verifies that an existing directory path resolves to `data.db` inside that directory,
    /// which is how directory-style `--db-path` inputs are interpreted.
    #[test]
    fn resolve_db_path_uses_data_db_inside_existing_directory() -> anyhow::Result<()> {
        let dir = unique_temp_db("custom-dir-existing");
        fs::create_dir_all(&dir)?;

        let resolved = resolve_db_path(Some(&dir))?;

        assert_eq!(resolved, dir.join("data.db"));
        assert!(resolved.parent().unwrap().exists());
        fs::remove_dir_all(dir)?;
        Ok(())
    }

    /// Verifies that a missing directory-like path is created and then resolved to `data.db`
    /// inside it, which covers the user flow of pointing at a new storage directory.
    #[test]
    fn resolve_db_path_creates_missing_directory_and_uses_data_db() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("vigil-custom-dir-missing-{}", Uuid::new_v4()));

        let resolved = resolve_db_path(Some(&dir))?;

        assert_eq!(resolved, dir.join("data.db"));
        assert!(dir.exists());
        fs::remove_dir_all(dir)?;
        Ok(())
    }

    /// Verifies that schema setup creates metadata and the default source row by opening a
    /// fresh database, running setup, and querying the resulting tables directly.
    #[test]
    fn setup_database_creates_metadata_tables() -> anyhow::Result<()> {
        let path = unique_temp_db("schema");
        let conn = build_test_db(&path)?;

        let schema_version: String = conn.query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(schema_version, SCHEMA_VERSION.to_string());

        let source = get_source(&conn, DEFAULT_SOURCE_ID)?;
        assert_eq!(source.id, DEFAULT_SOURCE_ID);
        assert!(!source.source_uuid.is_empty());
        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }

    /// Verifies that snapshot export writes both copied data and export metadata by exporting
    /// a seeded database and asserting on the snapshot's bucket rows and export record.
    #[test]
    fn export_database_creates_snapshot_with_export_metadata() -> anyhow::Result<()> {
        let source_path = unique_temp_db("export-source");
        let export_path = unique_temp_db("export-snapshot");
        let conn = build_test_db(&source_path)?;
        insert_input_buckets(&conn, &[sample_input_row()])?;

        let export = export_database(&source_path, &export_path)?;
        let snapshot = open_con_at(&export_path)?;
        let metadata = latest_export_metadata(&snapshot)?;

        assert_eq!(metadata.export_uuid, export.export_uuid);
        assert_eq!(
            scalar_query_u64(&snapshot, "SELECT COUNT(*) FROM input_buckets")?,
            1
        );

        drop(snapshot);
        drop(conn);
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        Ok(())
    }

    /// Verifies duplicate detection by recording an import with the same export UUID and hash,
    /// then checking that a later dry-run import is flagged as a duplicate instead of mergeable.
    #[test]
    fn plan_import_marks_duplicate_snapshot() -> anyhow::Result<()> {
        let destination_path = unique_temp_db("import-dest");
        let source_path = unique_temp_db("import-source");
        let export_path = unique_temp_db("import-export");

        let destination = build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        insert_input_buckets(&source, &[sample_input_row()])?;

        let export = export_database(&source_path, &export_path)?;
        destination.execute(
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
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
            ",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                export.export_uuid,
                "source-uuid",
                Utc::now().to_rfc3339(),
                Utc::now().to_rfc3339(),
                file_sha256(&export_path)?,
                SCHEMA_VERSION,
            ],
        )?;

        let plan = plan_import(&destination_path, &export_path)?;
        assert!(plan.duplicate_import);

        drop(source);
        drop(destination);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        Ok(())
    }

    /// Verifies that import merges same-source buckets and records import history by exporting
    /// a source snapshot, importing it, and asserting on merged totals plus metadata rows.
    #[test]
    fn import_snapshot_merges_rows_and_records_history() -> anyhow::Result<()> {
        let destination_path = unique_temp_db("merge-dest");
        let source_path = unique_temp_db("merge-source");
        let export_path = unique_temp_db("merge-export");

        let destination = build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        let destination_source_uuid: String = destination.query_row(
            "SELECT source_uuid FROM sources WHERE id = ?1",
            [DEFAULT_SOURCE_ID],
            |row| row.get(0),
        )?;
        source.execute(
            "UPDATE sources SET source_uuid = ?1 WHERE id = ?2",
            rusqlite::params![destination_source_uuid, DEFAULT_SOURCE_ID],
        )?;
        insert_input_buckets(&destination, &[sample_input_row()])?;
        insert_input_buckets(
            &source,
            &[InputBucketRecord {
                key_presses: 9,
                left_clicks: 4,
                ..sample_input_row()
            }],
        )?;
        insert_focus_buckets(&source, &[sample_focus_row()])?;

        export_database(&source_path, &export_path)?;
        let result = import_snapshot(&destination_path, &export_path, Some("sync test"))?;

        let merged = open_con_at(&destination_path)?;
        let stored = merged.query_row(
            "SELECT left_clicks, key_presses FROM input_buckets",
            [],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
        )?;
        let imports_count = scalar_query_u64(&merged, "SELECT COUNT(*) FROM imports")?;
        let focus_count = scalar_query_u64(&merged, "SELECT COUNT(*) FROM focus_buckets")?;

        assert_eq!(stored, (6, 14));
        assert_eq!(imports_count, 1);
        assert_eq!(focus_count, 1);
        assert!(result.destination_backup_path.exists());

        drop(merged);
        drop(source);
        drop(destination);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        fs::remove_file(result.destination_backup_path)?;
        Ok(())
    }

    /// Verifies that rows from a different source UUID stay separate after import by importing
    /// the same bucket shape from another source and asserting the destination keeps two rows.
    #[test]
    fn import_snapshot_with_different_source_uuid_keeps_rows_separate() -> anyhow::Result<()> {
        let destination_path = unique_temp_db("multi-source-dest");
        let source_path = unique_temp_db("multi-source-source");
        let export_path = unique_temp_db("multi-source-export");

        let destination = build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        insert_input_buckets(&destination, &[sample_input_row()])?;
        insert_input_buckets(&source, &[sample_input_row()])?;

        export_database(&source_path, &export_path)?;
        import_snapshot(&destination_path, &export_path, Some("new source"))?;

        let merged = open_con_at(&destination_path)?;
        let source_count = scalar_query_u64(&merged, "SELECT COUNT(*) FROM sources")?;
        let input_count = scalar_query_u64(&merged, "SELECT COUNT(*) FROM input_buckets")?;

        assert_eq!(source_count, 2);
        assert_eq!(input_count, 2);

        drop(merged);
        drop(source);
        drop(destination);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        Ok(())
    }

    /// Verifies that corrupted snapshot files fail during planning by writing invalid bytes
    /// to a temp file and checking that the planner reports a source snapshot failure.
    #[test]
    fn plan_import_rejects_corrupted_snapshot_file() -> anyhow::Result<()> {
        let destination_path = unique_temp_db("corrupt-dest");
        let source_path = unique_temp_db("corrupt-export");
        build_test_db(&destination_path)?;
        fs::write(&source_path, b"not-a-sqlite-database")?;

        let err = plan_import(&destination_path, &source_path).unwrap_err();
        assert!(
            err.to_string().contains("source snapshot"),
            "expected error mentioning 'source snapshot', got: {err}"
        );

        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        Ok(())
    }

    /// Verifies that import planning rejects mismatched schema versions by mutating the
    /// exported snapshot metadata and asserting the planner refuses to proceed.
    #[test]
    fn plan_import_rejects_schema_version_mismatch() -> anyhow::Result<()> {
        let destination_path = unique_temp_db("schema-mismatch-dest");
        let source_path = unique_temp_db("schema-mismatch-source");
        let export_path = unique_temp_db("schema-mismatch-export");

        build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        insert_input_buckets(&source, &[sample_input_row()])?;
        export_database(&source_path, &export_path)?;

        let export_conn = open_con_at(&export_path)?;
        export_conn.execute(
            "UPDATE schema_meta SET value = '999' WHERE key = 'schema_version'",
            [],
        )?;

        let err = plan_import(&destination_path, &export_path).unwrap_err();
        assert!(err.to_string().contains("schema version"));

        drop(export_conn);
        drop(source);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        Ok(())
    }

    /// Verifies that overlapping input and focus buckets merge only the overlapping rows while
    /// preserving distinct later buckets, using one shared-source snapshot with partial overlap.
    #[test]
    fn import_snapshot_handles_partial_bucket_and_window_overlaps() -> anyhow::Result<()> {
        let destination_path = unique_temp_db("partial-overlap-dest");
        let source_path = unique_temp_db("partial-overlap-source");
        let export_path = unique_temp_db("partial-overlap-export");

        let destination = build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        let destination_source_uuid: String = destination.query_row(
            "SELECT source_uuid FROM sources WHERE id = ?1",
            [DEFAULT_SOURCE_ID],
            |row| row.get(0),
        )?;
        source.execute(
            "UPDATE sources SET source_uuid = ?1 WHERE id = ?2",
            rusqlite::params![destination_source_uuid, DEFAULT_SOURCE_ID],
        )?;

        insert_input_buckets(&destination, &[sample_input_row()])?;
        insert_focus_buckets(&destination, &[sample_focus_row()])?;

        insert_input_buckets(&source, &[sample_input_row(), sample_second_input_row()])?;
        insert_focus_buckets(&source, &[sample_focus_row(), sample_second_focus_row()])?;

        export_database(&source_path, &export_path)?;
        import_snapshot(&destination_path, &export_path, None)?;

        let merged = open_con_at(&destination_path)?;
        let input_rows = scalar_query_u64(&merged, "SELECT COUNT(*) FROM input_buckets")?;
        let focus_rows = scalar_query_u64(&merged, "SELECT COUNT(*) FROM focus_buckets")?;

        // Overlapping input bucket: all counters should be summed.
        let (key_presses, left_clicks, mouse_distance_cm) = merged.query_row(
            "SELECT key_presses, left_clicks, mouse_distance_cm
             FROM input_buckets WHERE bucket_start_utc = ?1",
            [sample_input_row().bucket_start_utc.to_rfc3339()],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            },
        )?;

        // Overlapping focus bucket: focus_seconds should be summed.
        let focus_seconds: u64 = merged.query_row(
            "SELECT focus_seconds FROM focus_buckets WHERE window_title = 'Docs'",
            [],
            |row| row.get(0),
        )?;

        assert_eq!(input_rows, 2);
        assert_eq!(focus_rows, 2);
        assert_eq!(key_presses, 10); // 5 + 5
        assert_eq!(left_clicks, 4); // 2 + 2
        assert!((mouse_distance_cm - 6.0).abs() < 1e-6); // 3.0 + 3.0
        assert_eq!(focus_seconds, 240); // 120 + 120

        drop(merged);
        drop(source);
        drop(destination);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        Ok(())
    }

    /// Verifies that duplicate detection prioritizes export UUID over file hash changes by
    /// mutating snapshot metadata after import and checking the plan still rejects reimport.
    #[test]
    fn duplicate_detection_prefers_export_uuid_even_if_snapshot_hash_changes() -> anyhow::Result<()>
    {
        let destination_path = unique_temp_db("duplicate-export-uuid-dest");
        let source_path = unique_temp_db("duplicate-export-uuid-source");
        let export_path = unique_temp_db("duplicate-export-uuid-export");

        let destination = build_test_db(&destination_path)?;
        let source = build_test_db(&source_path)?;
        insert_input_buckets(&source, &[sample_input_row()])?;
        export_database(&source_path, &export_path)?;
        import_snapshot(&destination_path, &export_path, Some("first import"))?;

        let export_conn = open_con_at(&export_path)?;
        export_conn.execute(
            "UPDATE exports SET notes = 'mutated after import' WHERE id = (SELECT MAX(id) FROM exports)",
            [],
        )?;

        let plan = plan_import(&destination_path, &export_path)?;
        assert!(plan.duplicate_import);
        assert!(plan
            .duplicate_reason
            .unwrap_or_default()
            .contains("export UUID"));

        drop(export_conn);
        drop(source);
        drop(destination);
        fs::remove_file(destination_path)?;
        fs::remove_file(source_path)?;
        fs::remove_file(export_path)?;
        let backup_dir = std::env::temp_dir();
        for entry in fs::read_dir(backup_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.contains("duplicate-export-uuid-dest") && name.contains("pre-import") {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }

    /// Verifies that begin_session creates a session row and end_session closes it by
    /// querying the sessions table directly and checking the ended_at_utc state.
    #[test]
    fn begin_and_end_session_persist_session_rows() -> anyhow::Result<()> {
        let path = unique_temp_db("session-lifecycle");
        let conn = build_test_db(&path)?;

        let first = begin_session(&conn, DEFAULT_SOURCE_ID, "linux")?;
        end_session(&conn, &first)?;
        let second = begin_session(&conn, DEFAULT_SOURCE_ID, "linux")?;

        let count: u64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?;
        assert_eq!(count, 2);

        // First session should be closed.
        let first_ended: Option<String> = conn.query_row(
            "SELECT ended_at_utc FROM sessions WHERE session_uuid = ?1",
            [&first],
            |row| row.get(0),
        )?;
        assert!(first_ended.is_some());

        // Second session should be open.
        let second_ended: Option<String> = conn.query_row(
            "SELECT ended_at_utc FROM sessions WHERE session_uuid = ?1",
            [&second],
            |row| row.get(0),
        )?;
        assert!(second_ended.is_none());

        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }

    /// Verifies that focus bucket aggregation by app identifier works correctly by inserting
    /// multiple focus rows across apps and asserting the per-app totals via direct SQL.
    #[test]
    fn focus_buckets_aggregate_by_app_identifier() -> anyhow::Result<()> {
        let path = unique_temp_db("focus-agg");
        let conn = build_test_db(&path)?;
        insert_focus_buckets(
            &conn,
            &[
                sample_focus_row(), // firefox, 120s
                FocusBucketRecord {
                    app_identifier: "firefox".to_string(),
                    focus_seconds: 30,
                    ..sample_second_focus_row()
                },
                FocusBucketRecord {
                    app_identifier: "code".to_string(),
                    window_title: "Workspace".to_string(),
                    window_class: "code".to_string(),
                    focus_seconds: 10,
                    ..sample_second_focus_row()
                },
            ],
        )?;

        let firefox_total: u64 = conn.query_row(
            "SELECT SUM(focus_seconds) FROM focus_buckets WHERE app_identifier = 'firefox'",
            [],
            |row| row.get(0),
        )?;
        let code_total: u64 = conn.query_row(
            "SELECT SUM(focus_seconds) FROM focus_buckets WHERE app_identifier = 'code'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(firefox_total, 150);
        assert_eq!(code_total, 10);

        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }

    /// Verifies that daily analytics merge input and focus totals per local day and source by
    /// inserting both bucket kinds and asserting the joined daily summary row.
    #[test]
    fn daily_activity_report_groups_metrics_by_day_and_source() -> anyhow::Result<()> {
        let path = unique_temp_db("daily-report");
        let conn = build_test_db(&path)?;
        let input_rows = report_test_input_rows();
        let focus_rows = report_test_focus_rows();
        insert_input_buckets(&conn, &input_rows)?;
        insert_focus_buckets(&conn, &focus_rows)?;

        let rows = daily_activity_report(&conn, 30)?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].local_date, input_rows[0].local_date);
        assert_eq!(rows[0].key_presses, 12);
        assert_eq!(rows[0].left_clicks, 5);
        assert_eq!(rows[0].focus_seconds, 165);
        assert!((rows[0].mouse_distance_cm - 4.5).abs() < 1e-6);

        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }

    /// Verifies that session duration computation in SQLite is correct by inserting two sessions
    /// with known timestamps and asserting the derived totals via direct aggregate query.
    #[test]
    fn session_duration_aggregates_correctly_in_sql() -> anyhow::Result<()> {
        let path = unique_temp_db("session-durations");
        let conn = build_test_db(&path)?;

        // Two closed sessions: 60 s and 120 s.
        conn.execute_batch(&format!(
            "
            INSERT INTO sessions (source_id, started_at_utc, ended_at_utc, session_uuid, platform)
            VALUES ({id}, '2026-04-18T10:00:00Z', '2026-04-18T10:01:00Z', 'dur-uuid-1', 'linux');
            INSERT INTO sessions (source_id, started_at_utc, ended_at_utc, session_uuid, platform)
            VALUES ({id}, '2026-04-18T10:02:00Z', '2026-04-18T10:04:00Z', 'dur-uuid-2', 'linux');
            ",
            id = DEFAULT_SOURCE_ID
        ))?;

        let (count, total, longest): (u64, u64, u64) = conn.query_row(
            "
            SELECT COUNT(*),
                   COALESCE(SUM(CAST(strftime('%s', ended_at_utc) AS INTEGER)
                                - CAST(strftime('%s', started_at_utc) AS INTEGER)), 0),
                   COALESCE(MAX(CAST(strftime('%s', ended_at_utc) AS INTEGER)
                                - CAST(strftime('%s', started_at_utc) AS INTEGER)), 0)
            FROM sessions
            WHERE ended_at_utc IS NOT NULL
            ",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(count, 2);
        assert_eq!(total, 180); // 60 + 120
        assert_eq!(longest, 120);

        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }

    /// Verifies that inserting an input bucket row twice with the same key accumulates counters
    /// rather than overwriting them, which is the core UPSERT contract for local collection.
    #[test]
    fn insert_input_buckets_upsert_sums_existing_bucket() -> anyhow::Result<()> {
        let path = unique_temp_db("upsert-input");
        let conn = build_test_db(&path)?;

        let row = sample_input_row();
        insert_input_buckets(&conn, &[row.clone()])?;
        insert_input_buckets(
            &conn,
            &[InputBucketRecord {
                key_presses: 3,
                left_clicks: 1,
                mouse_distance_cm: 2.0,
                ..row
            }],
        )?;

        let (key_presses, left_clicks, mouse_distance_cm) = conn.query_row(
            "SELECT key_presses, left_clicks, mouse_distance_cm FROM input_buckets",
            [],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            },
        )?;
        let row_count = scalar_query_u64(&conn, "SELECT COUNT(*) FROM input_buckets")?;

        assert_eq!(row_count, 1); // no duplicate rows
        assert_eq!(key_presses, 8); // 5 + 3
        assert_eq!(left_clicks, 3); // 2 + 1
        assert!((mouse_distance_cm - 5.0).abs() < 1e-6); // 3.0 + 2.0

        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }

    /// Verifies that begin_session closes any previously open session for the same source before
    /// opening a new one, preventing phantom open sessions from accumulating across restarts.
    #[test]
    fn begin_session_closes_previously_open_sessions() -> anyhow::Result<()> {
        let path = unique_temp_db("begin-session-closes");
        let conn = build_test_db(&path)?;

        let first = begin_session(&conn, DEFAULT_SOURCE_ID, "linux")?;
        // At this point the first session is open.
        let open_count: u64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE ended_at_utc IS NULL",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(open_count, 1);

        // Starting a second session must close the first.
        let _second = begin_session(&conn, DEFAULT_SOURCE_ID, "linux")?;
        let first_ended: Option<String> = conn
            .query_row(
                "SELECT ended_at_utc FROM sessions WHERE session_uuid = ?1",
                [&first],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        assert!(
            first_ended.is_some(),
            "first session should have been closed"
        );

        let still_open: u64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE ended_at_utc IS NULL",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(still_open, 1, "only the new session should remain open");

        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }

    /// Verifies that end_session does not overwrite ended_at_utc when a session is already
    /// closed, which is the COALESCE guard that prevents double-closing a session.
    #[test]
    fn end_session_does_not_update_already_ended_session() -> anyhow::Result<()> {
        let path = unique_temp_db("end-session-idempotent");
        let conn = build_test_db(&path)?;

        let uuid = begin_session(&conn, DEFAULT_SOURCE_ID, "linux")?;
        end_session(&conn, &uuid)?;

        let first_end: String = conn.query_row(
            "SELECT ended_at_utc FROM sessions WHERE session_uuid = ?1",
            [&uuid],
            |row| row.get(0),
        )?;

        // Calling end_session again must not change the recorded end time.
        end_session(&conn, &uuid)?;
        let second_end: String = conn.query_row(
            "SELECT ended_at_utc FROM sessions WHERE session_uuid = ?1",
            [&uuid],
            |row| row.get(0),
        )?;

        assert_eq!(first_end, second_end);

        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }

    /// Verifies that daily_activity_report includes days that have focus data but no input data,
    /// covering the BTreeMap merge path where a source_uuid key is new to the map.
    #[test]
    fn daily_activity_report_includes_focus_only_day() -> anyhow::Result<()> {
        let path = unique_temp_db("focus-only-day");
        let conn = build_test_db(&path)?;
        let focus_rows = report_test_focus_rows();

        // Insert only focus data — no input_buckets rows at all.
        insert_focus_buckets(&conn, &[focus_rows[0].clone()])?;

        let rows = daily_activity_report(&conn, 30)?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].local_date, focus_rows[0].local_date);
        assert_eq!(rows[0].focus_seconds, 120);
        // Input counters must default to zero, not be missing.
        assert_eq!(rows[0].key_presses, 0);
        assert_eq!(rows[0].left_clicks, 0);
        assert!((rows[0].mouse_distance_cm - 0.0).abs() < 1e-6);

        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }

    /// Verifies that file_sha256 produces the same hash when called twice on the same file,
    /// since the hash is used as an import dedup key and must be deterministic.
    #[test]
    fn file_sha256_is_deterministic() -> anyhow::Result<()> {
        let path = unique_temp_db("sha256-test");
        build_test_db(&path)?;

        let hash1 = file_sha256(&path)?;
        let hash2 = file_sha256(&path)?;

        assert_eq!(hash1, hash2);
        assert!(!hash1.is_empty());
        assert_eq!(hash1.len(), 64, "SHA-256 hex digest must be 64 characters");

        fs::remove_file(path)?;
        Ok(())
    }
}
