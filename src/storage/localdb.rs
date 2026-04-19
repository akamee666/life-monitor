mod analytics;
mod config;
mod export;
mod import;
mod integrity;
mod rows;
mod schema;

#[allow(unused_imports)]
pub use analytics::{
    app_activity_report, begin_session, current_open_session, end_session, session_report,
    AppActivityRow, SessionRow,
};
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
    use chrono::{TimeZone, Utc};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use uuid::Uuid;

    fn unique_temp_db(name: &str) -> PathBuf {
        let suffix = Uuid::new_v4();
        std::env::temp_dir().join(format!("life-monitor-{name}-{suffix}.db"))
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
        std::env::set_var("LIFE_MONITOR_DATA_DIR", &data_dir);

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

        std::env::remove_var("LIFE_MONITOR_DATA_DIR");
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
        let dir = std::env::temp_dir().join(format!(
            "life-monitor-custom-dir-missing-{}",
            Uuid::new_v4()
        ));

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
    /// to a temp file and checking that the planner reports an open or integrity failure.
    #[test]
    fn plan_import_rejects_corrupted_snapshot_file() -> anyhow::Result<()> {
        let destination_path = unique_temp_db("corrupt-dest");
        let source_path = unique_temp_db("corrupt-export");
        build_test_db(&destination_path)?;
        fs::write(&source_path, b"not-a-sqlite-database")?;

        let err = plan_import(&destination_path, &source_path).unwrap_err();
        assert!(
            err.to_string().contains("Failed to open source snapshot")
                || err.to_string().contains("integrity")
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
        let overlapping = merged.query_row(
            "SELECT key_presses FROM input_buckets WHERE bucket_start_utc = ?1",
            [sample_input_row().bucket_start_utc.to_rfc3339()],
            |row| row.get::<_, u64>(0),
        )?;

        assert_eq!(input_rows, 2);
        assert_eq!(focus_rows, 2);
        assert_eq!(overlapping, 10);

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

    /// Verifies that session reporting returns both open and closed sessions in reverse
    /// chronology by creating one ended session and one current session in a real database.
    #[test]
    fn session_report_returns_closed_and_open_sessions() -> anyhow::Result<()> {
        let path = unique_temp_db("session-report");
        let conn = build_test_db(&path)?;

        let first = begin_session(&conn, DEFAULT_SOURCE_ID, "windows")?;
        end_session(&conn, &first)?;
        let second = begin_session(&conn, DEFAULT_SOURCE_ID, "windows")?;

        let sessions = session_report(&conn, 30)?;
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_uuid, second);
        assert_eq!(sessions[0].ended_at_utc, None);
        assert!(sessions[0].duration_seconds.is_some());
        assert_eq!(sessions[1].session_uuid, first);
        assert!(sessions[1].ended_at_utc.is_some());

        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }

    /// Verifies that app activity reporting aggregates focus time by app identifier by loading
    /// multiple focus rows and asserting the grouped totals and ordering from the query result.
    #[test]
    fn app_activity_report_aggregates_focus_seconds_by_app() -> anyhow::Result<()> {
        let path = unique_temp_db("app-report");
        let conn = build_test_db(&path)?;
        insert_focus_buckets(
            &conn,
            &[
                sample_focus_row(),
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

        let rows = app_activity_report(&conn, 30)?;
        assert_eq!(rows[0].app_identifier, "firefox");
        assert_eq!(rows[0].focus_seconds, 150);
        assert_eq!(rows[1].app_identifier, "code");
        assert_eq!(rows[1].focus_seconds, 10);

        drop(conn);
        fs::remove_file(path)?;
        Ok(())
    }
}
