use crate::{keylogger::KeyLogger, processinfo::ProcessInfo};
use rusqlite::{params, Connection};
use std::{env, fs, path::PathBuf};
use tracing::*;

pub fn send_to_input_table(
    conn: &Connection,
    logger_data: &KeyLogger,
) -> Result<(), Box<dyn std::error::Error>> {
    debug!("Sending data to inputs table");

    let query_update = "
        UPDATE input_logs
        SET left_clicks = ?,
            right_clicks = ?,
            middle_clicks = ?,
            keys_pressed = ?,
            mouse_moved_cm = ?;
    ";

    conn.execute(
        query_update,
        params![
            logger_data.left_clicks,
            logger_data.right_clicks,
            logger_data.middle_clicks,
            logger_data.keys_pressed,
            logger_data.mouse_moved_cm,
        ],
    )?;

    Ok(())
}

pub fn get_input_data(conn: &Connection) -> Result<KeyLogger, Box<dyn std::error::Error>> {
    let query = "
    SELECT left_clicks, right_clicks, middle_clicks, keys_pressed, mouse_moved_cm 
    FROM input_logs 
    LIMIT 1;
    ";

    let mut stmt = conn.prepare(query)?;

    // Assuming there is only one row
    let row = stmt.query_row([], |row| {
        Ok(KeyLogger {
            left_clicks: row.get(0)?,
            right_clicks: row.get(1)?,
            middle_clicks: row.get(2)?,
            keys_pressed: row.get(3)?,
            mouse_moved_cm: row.get(4)?,
            pixels_moved: 0.0, // Default or computed value
            //FIX: me
            mouse_dpi: 0,
            ..Default::default()
        })
    })?;

    Ok(row)
}

pub fn get_process_data(conn: &Connection) -> Result<Vec<ProcessInfo>, Box<dyn std::error::Error>> {
    let query = "
        SELECT process_name, seconds_spent, instance, window_class
        FROM time_wasted;
    ";

    let mut stmt = conn.prepare(query)?;

    let process_vec = stmt
        .query_map([], |row| {
            Ok(ProcessInfo {
                name: row.get(0)?,
                time_spent: row.get(1)?,
                instance: row.get(2)?,
                window_class: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<ProcessInfo>, _>>()?;

    Ok(process_vec)
}

pub fn send_to_process_table(
    conn: &Connection,
    process_vec: &Vec<ProcessInfo>,
) -> Result<(), Box<dyn std::error::Error>> {
    debug!("Sending Data to processes table");

    for process in process_vec {
        // Check if the process already exists in the table
        let query_check = "SELECT 1 FROM time_wasted WHERE process_name = ? LIMIT 1;";
        let mut stmt_check = conn.prepare(query_check)?;
        let exists = stmt_check.exists(params![&process.name])?;

        if exists {
            debug!(
                "Already have this program in processes table, updating values for {}",
                process.name
            );
            let query_update = "UPDATE time_wasted SET seconds_spent = ?, instance = ?, window_class = ? WHERE process_name = ?;";
            conn.execute(
                query_update,
                params![
                    process.time_spent,
                    process.instance,
                    process.window_class,
                    process.name
                ],
            )?;
        } else {
            let query_insert = "INSERT INTO time_wasted (process_name, seconds_spent, instance, window_class) VALUES (?, ?, ?, ?);";
            conn.execute(
                query_insert,
                params![
                    process.name,
                    process.time_spent,
                    process.instance,
                    process.window_class
                ],
            )?;
        }
    }
    Ok(())
}

fn get_path() -> Result<(PathBuf, bool), std::io::Error> {
    use std::io;
    let mut isnew = false;
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

        // I'll create tables in open_con function to reuse the connection so i don't open it two
        // times.
        if !path.exists() {
            debug!("New database at: {}", path.display());

            isnew = true;
        }

        if let Some(parent_dir) = path.parent() {
            debug!("Parent from database: {}", parent_dir.display());
            fs::create_dir_all(parent_dir)?;
        }
        (path, isnew)
    } else if cfg!(target_os = "linux") {
        let home_dir = env::var("HOME").map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, "HOME environment variable not set")
        })?;
        let mut path = PathBuf::from(home_dir);
        path.push(".local");
        path.push("share");
        path.push("akame_monitor");
        path.push("tracked_data.db");

        // I'll create tables in open_con function to reuse the connection so i don't open it two
        // times.
        if !path.exists() {
            debug!("New database at: {}", path.display());
            isnew = true;
        }

        if let Some(parent_dir) = path.parent() {
            debug!("Parent from database: {}", parent_dir.display());
            fs::create_dir_all(parent_dir)?;
        }

        (path, isnew)
    } else {
        // Handle other OSes if needed
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unsupported operating system",
        ));
    };

    Ok(path)
}

pub fn open_con() -> Result<Connection, Box<dyn std::error::Error>> {
    let (path, isnew) = get_path()?;

    // Open the database connection, create if do not exist
    let conn = Connection::open(&path)?;

    if isnew {
        debug!("Database is new, creating tables...");

        // Create input_logs table
        let query_create_input_logs_table = "
            CREATE TABLE input_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                left_clicks INTEGER NOT NULL,
                right_clicks INTEGER NOT NULL,
                middle_clicks INTEGER NOT NULL,
                keys_pressed INTEGER NOT NULL,
                mouse_moved_cm INTEGER NOT NULL
            );
        ";
        conn.execute(query_create_input_logs_table, [])?;

        // Insert initial row into input_logs table
        let query_insert_initial_rows = "
            INSERT INTO input_logs (left_clicks, right_clicks, middle_clicks, keys_pressed, mouse_moved_cm)
            VALUES (0, 0, 0, 0, 0);
        ";
        conn.execute(query_insert_initial_rows, [])?;

        // Create time_wasted table
        let query_create_time_wasted_table = "
            CREATE TABLE time_wasted (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                process_name TEXT NOT NULL,
                seconds_spent INTEGER NOT NULL,
                instance TEXT,
                window_class TEXT NOT NULL
            );
        ";
        conn.execute(query_create_time_wasted_table, [])?;

        debug!("Tables created successfully.");
    }
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    // Helper function to create an in-memory database for testing
    fn create_test_db() -> Result<Connection, Box<dyn std::error::Error>> {
        let conn = Connection::open_in_memory()?;

        // Create the necessary tables for testing
        conn.execute(
            "CREATE TABLE input_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            left_clicks INTEGER NOT NULL,
            right_clicks INTEGER NOT NULL,
            middle_clicks INTEGER NOT NULL,
            keys_pressed INTEGER NOT NULL,
            mouse_moved_cm INTEGER NOT NULL
        )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE time_wasted (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            process_name TEXT NOT NULL,
            seconds_spent INTEGER NOT NULL,
            instance TEXT,
            window_class TEXT NOT NULL
        )",
            [],
        )?;

        Ok(conn)
    }

    #[test]
    fn test_send_to_input_table() -> Result<(), Box<dyn Error>> {
        let conn = create_test_db()?;

        let logger_data = KeyLogger {
            left_clicks: 10,
            right_clicks: 5,
            middle_clicks: 2,
            keys_pressed: 100,
            mouse_moved_cm: 50.0,
            pixels_moved: 0.0,
            mouse_dpi: 1600,
            ..Default::default()
        };

        send_to_input_table(&conn, &logger_data)?;

        // Verify the data was inserted correctly
        let row: (i32, i32, i32, i32, i32) = conn.query_row(
        "SELECT left_clicks, right_clicks, middle_clicks, keys_pressed, mouse_moved_cm FROM input_logs",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )?;

        assert_eq!(row, (10, 5, 2, 100, 50));

        Ok(())
    }

    #[test]
    fn test_get_input_data() -> Result<(), Box<dyn Error>> {
        let conn = create_test_db()?;

        // Insert test data
        conn.execute(
        "INSERT INTO input_logs (left_clicks, right_clicks, middle_clicks, keys_pressed, mouse_moved_cm) VALUES (?, ?, ?, ?, ?)",
        params![5, 3, 1, 50, 25],
    )?;

        let result = get_input_data(&conn)?;

        assert_eq!(result.left_clicks, 5);
        assert_eq!(result.right_clicks, 3);
        assert_eq!(result.middle_clicks, 1);
        assert_eq!(result.keys_pressed, 50);
        assert_eq!(result.mouse_moved_cm, 25.0);

        Ok(())
    }

    #[test]
    fn test_get_process_data() -> Result<(), Box<dyn Error>> {
        let conn = create_test_db()?;

        // Insert test data
        conn.execute(
        "INSERT INTO time_wasted (process_name, seconds_spent, instance, window_class) VALUES (?, ?, ?, ?)",
        params!["test_process", 100, "test_instance", "test_class"],
    )?;

        let result = get_process_data(&conn)?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "test_process");
        assert_eq!(result[0].time_spent, 100);
        assert_eq!(result[0].instance, "test_instance");
        assert_eq!(result[0].window_class, "test_class");

        Ok(())
    }

    #[test]
    fn test_send_to_process_table() -> Result<(), Box<dyn Error>> {
        let conn = create_test_db()?;

        let process_vec = vec![ProcessInfo {
            name: "test_process".to_string(),
            time_spent: 100,
            instance: "test_instance".to_string(),
            window_class: "test_class".to_string(),
        }];

        send_to_process_table(&conn, &process_vec)?;

        // Verify the data was inserted correctly
        let row: (String, i32, String, String) = conn.query_row(
            "SELECT process_name, seconds_spent, instance, window_class FROM time_wasted",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;

        assert_eq!(
            row,
            (
                "test_process".to_string(),
                100,
                "test_instance".to_string(),
                "test_class".to_string()
            )
        );

        Ok(())
    }
}
