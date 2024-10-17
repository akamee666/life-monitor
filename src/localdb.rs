use crate::keylogger::KeyLogger;
use crate::ProcessInfo;

use rusqlite::params;
use rusqlite::Connection;
use rusqlite::OpenFlags;
use rusqlite::Result as SqlResult;

use std::env;
use std::fs;
use std::path::PathBuf;

use tracing::*;

#[cfg(target_os = "linux")]
use crate::linux::util::MouseSettings;

#[cfg(target_os = "windows")]
use crate::win::util::MouseSettings;

pub fn clean_database() -> std::io::Result<()> {
    let path = find_path()?;
    fs::remove_file(path.clone())?;
    info!("Database deleted at {} with success!", path.display());
    Ok(())
}

pub fn update_keyst(conn: &Connection, logger_data: &KeyLogger) -> SqlResult<()> {
    debug!("Updating keys table, current Keylogger: {:#?}", logger_data);
    let query_update = "
        UPDATE keys
        SET left_clicks = ?,
            right_clicks = ?,
            middle_clicks = ?,
            keys_pressed = ?,
            mouse_moved_cm = ?, 
            mouse_dpi = ? WHERE id = 1;
    ";

    conn.execute(
        query_update,
        params![
            logger_data.left_clicks,
            logger_data.right_clicks,
            logger_data.middle_clicks,
            logger_data.keys_pressed,
            logger_data.mouse_moved_cm,
            logger_data.mouse_settings.dpi,
        ],
    )?;

    Ok(())
}

pub fn get_keyst(conn: &Connection) -> SqlResult<KeyLogger> {
    let query = "
    SELECT left_clicks, right_clicks, middle_clicks, keys_pressed, mouse_moved_cm,mouse_dpi
    FROM keys
    LIMIT 1;
    ";

    let mut stmt = conn.prepare(query)?;

    // Assuming there is only one row
    let row = stmt.query_row([], |row| {
        let k = KeyLogger {
            left_clicks: row.get(0)?,
            right_clicks: row.get(1)?,
            middle_clicks: row.get(2)?,
            keys_pressed: row.get(3)?,
            mouse_moved_cm: row.get(4)?,
            mouse_settings: MouseSettings {
                dpi: row.get(5)?,
                ..Default::default()
            },
            ..Default::default()
        };
        Ok(k)
    })?;

    Ok(row)
}

// Need to be owned by the caller, Vec seems better in that case.
pub fn get_proct(conn: &Connection) -> SqlResult<Vec<ProcessInfo>> {
    let query = "
        SELECT window_name,window_time,window_instance, window_class
        FROM procs;
    ";

    let mut stmt = conn.prepare(query)?;

    let process_vec = stmt
        .query_map([], |row| {
            Ok(ProcessInfo {
                window_name: row.get(0)?,
                window_time: row.get(1)?,
                window_instance: row.get(2)?,
                window_class: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<ProcessInfo>, _>>()?;

    Ok(process_vec)
}

// Only borrowing and only reading the data, &[ProcessInfo] is better in that case.
pub fn update_proct(conn: &Connection, process_vec: &[ProcessInfo]) -> SqlResult<()> {
    for process in process_vec {
        // FIX:
        let query_check = "SELECT 1 FROM procs WHERE window_name = ? LIMIT 1;";
        let mut stmt_check = conn.prepare(query_check)?;
        let exists = stmt_check.exists(params![&process.window_name])?;

        if exists {
            let query_update = "UPDATE procs SET window_time = ?, window_instance = ?, window_class = ? WHERE process_name = ?;";
            conn.execute(
                query_update,
                params![
                    process.window_time,
                    process.window_instance,
                    process.window_class,
                    process.window_name
                ],
            )?;
        } else {
            let query_insert = "INSERT INTO procs (window_name,window_time,window_instance, window_class) VALUES (?, ?, ?, ?);";
            conn.execute(
                query_insert,
                params![
                    process.window_name,
                    process.window_time,
                    process.window_instance,
                    process.window_class
                ],
            )?;
        }
    }
    Ok(())
}

fn find_path() -> Result<PathBuf, std::io::Error> {
    use std::io;
    // Find a proper path to store the database in both os, create if already not exist
    let path = if cfg!(target_os = "windows") {
        let local_app_data = env::var("LOCALAPPDATA").map_err(|_| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "LOCALAPPDATA environment variable not set",
            )
        })?;
        let mut path = PathBuf::from(local_app_data);
        path.push("akame_monitor");
        path.push("tracked_data.db");

        path
    } else if cfg!(target_os = "linux") {
        let home_dir = env::var("HOME").map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, "HOME environment variable not set")
        })?;
        let mut path = PathBuf::from(home_dir);
        path.push(".local");
        path.push("share");
        path.push("akame_monitor");
        path.push("tracked_data.db");

        path
    } else {
        // Handle other OSes if needed
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unsupported operating system",
        ));
    };

    Ok(path)
}

pub fn open_con() -> SqlResult<Connection> {
    let path = match find_path() {
        Ok(path) => path,
        Err(e) => {
            error!("Could not find path for database file.\n Error: {e}");
            panic!();
        }
    };
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;
    initialize_database(&conn)?;
    Ok(conn)
}

fn initialize_database(conn: &Connection) -> SqlResult<()> {
    conn.execute(
        "
        CREATE TABLE IF NOT EXISTS keys (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            left_clicks INTEGER NOT NULL,
            right_clicks INTEGER NOT NULL,
            middle_clicks INTEGER NOT NULL,
            keys_pressed INTEGER NOT NULL,
            mouse_moved_cm INTEGER NOT NULL,
            mouse_dpi INTEGER NOT NULL
        );
    ",
        [],
    )?;

    conn.execute("
        INSERT OR IGNORE INTO keys (id, left_clicks, right_clicks, middle_clicks, keys_pressed, mouse_moved_cm, mouse_dpi)
        VALUES (1, 0, 0, 0, 0, 0, 0);
    ", [])?;

    conn.execute(
        "
        CREATE TABLE IF NOT EXISTS procs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            window_name TEXT NOT NULL,
            window_time INTEGER NOT NULL,
            window_instance TEXT,
            window_class TEXT NOT NULL
        );
    ",
        [],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Result as SqlResult;

    // Helper function to create an in-memory database for testing
    fn create_test_db() -> SqlResult<Connection> {
        let conn = Connection::open_in_memory()?;
        initialize_database(&conn)?;
        Ok(conn)
    }

    #[test]
    fn test_update_and_get_keyst() -> SqlResult<()> {
        let conn = create_test_db()?;

        let initial_logger_data = KeyLogger {
            left_clicks: 10,
            right_clicks: 5,
            middle_clicks: 2,
            keys_pressed: 100,
            mouse_moved_cm: 50.0,
            mouse_settings: MouseSettings {
                dpi: 1600,
                ..Default::default()
            },
            ..Default::default()
        };

        // Insert initial data
        update_keyst(&conn, &initial_logger_data)?;

        // Verify the data was inserted correctly
        let result = get_keyst(&conn)?;
        assert_eq!(result.left_clicks, 10);
        assert_eq!(result.right_clicks, 5);
        assert_eq!(result.middle_clicks, 2);
        assert_eq!(result.keys_pressed, 100);
        assert_eq!(result.mouse_moved_cm, 50.0);
        assert_eq!(result.mouse_settings.dpi, 1600);

        // Update with new data
        let updated_logger_data = KeyLogger {
            left_clicks: 15,
            right_clicks: 7,
            middle_clicks: 3,
            keys_pressed: 150,
            mouse_moved_cm: 75.0,
            mouse_settings: MouseSettings {
                dpi: 2400,
                ..Default::default()
            },
            ..Default::default()
        };

        update_keyst(&conn, &updated_logger_data)?;

        // Verify the data was updated correctly
        let updated_result = get_keyst(&conn)?;
        assert_eq!(updated_result.left_clicks, 15);
        assert_eq!(updated_result.right_clicks, 7);
        assert_eq!(updated_result.middle_clicks, 3);
        assert_eq!(updated_result.keys_pressed, 150);
        assert_eq!(updated_result.mouse_moved_cm, 75.0);
        assert_eq!(updated_result.mouse_settings.dpi, 2400);

        Ok(())
    }

    #[test]
    fn test_update_and_get_proct() -> SqlResult<()> {
        let conn = create_test_db()?;

        let initial_processes = vec![
            ProcessInfo {
                window_name: "test_process1".to_string(),
                window_time: 100,
                window_instance: "test_instance1".to_string(),
                window_class: "test_class1".to_string(),
            },
            ProcessInfo {
                window_name: "test_process2".to_string(),
                window_time: 200,
                window_instance: "test_instance2".to_string(),
                window_class: "test_class2".to_string(),
            },
        ];

        update_proct(&conn, &initial_processes)?;

        // Verify the data was inserted correctly
        let result = get_proct(&conn)?;
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].window_name, "test_process1");
        assert_eq!(result[0].window_time, 100);
        assert_eq!(result[1].window_name, "test_process2");
        assert_eq!(result[1].window_time, 200);

        // Update existing process and add a new one
        let updated_processes = vec![
            ProcessInfo {
                window_name: "test_process1".to_string(),
                window_time: 150,
                window_instance: "test_instance1_updated".to_string(),
                window_class: "test_class1".to_string(),
            },
            ProcessInfo {
                window_name: "test_process3".to_string(),
                window_time: 300,
                window_instance: "test_instance3".to_string(),
                window_class: "test_class3".to_string(),
            },
        ];

        update_proct(&conn, &updated_processes)?;

        // Verify the data was updated correctly
        let updated_result = get_proct(&conn)?;
        assert_eq!(updated_result.len(), 3);
        let process1 = updated_result
            .iter()
            .find(|p| p.window_name == "test_process1")
            .unwrap();
        assert_eq!(process1.window_time, 150);
        assert_eq!(process1.window_instance, "test_instance1_updated");

        let process3 = updated_result
            .iter()
            .find(|p| p.window_name == "test_process3")
            .unwrap();
        assert_eq!(process3.window_time, 300);

        Ok(())
    }
}
