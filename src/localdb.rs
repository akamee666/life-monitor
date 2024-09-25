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
        //FIX: SAVE CALIBRATION.
        Ok(KeyLogger {
            left_clicks: row.get(0)?,
            right_clicks: row.get(1)?,
            middle_clicks: row.get(2)?,
            keys_pressed: row.get(3)?,
            mouse_moved_cm: row.get(4)?,
            pixels_moved: 0.0, // Default or computed value
            calibration_pixels: 0.0,
            calibration_distance_cm: 0.0,
            mouse_dpi: 0.0,
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

fn get_db_path() -> Result<PathBuf, std::io::Error> {
    use std::io;
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

pub fn open_con() -> Result<Connection, Box<dyn std::error::Error>> {
    let path = get_db_path()?;

    // Create parent directory if it does not exist
    if let Some(parent_dir) = path.parent() {
        fs::create_dir_all(parent_dir)?;
    }

    // Open the database connection, create if do not exist
    let conn = Connection::open(&path)?;

    // Check if the database is new and create tables if it is
    if fs::metadata(&path).is_err() {
        debug!("Database does not exist, creating tables.");

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
    } else {
        debug!("Database exists, opening connection.");
    }

    Ok(conn)
}
