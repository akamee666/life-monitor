use crate::keylogger::KeyLogger;
use sqlite::Connection;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;
use tracing::error;

fn get_db_conn() -> Result<Connection, Box<dyn std::error::Error>> {
    debug!("Sarting db connection");

    let local_app_data = env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\Temp".into());
    let mut path = PathBuf::from(&local_app_data);
    path.push("akame_monitor");
    path.push("tracked_data.db");

    debug!("Path for db: {:?}", path);

    if let Some(parent_dir) = path.parent() {
        fs::create_dir_all(parent_dir)?;
    }

    let conn = if !Path::new(&path).exists() {
        let conn = Connection::open(&path)?;
        debug!("Database created at: {}", path.display());
        let query_create_tables = "CREATE TABLE input_logs (id INTEGER PRIMARY KEY AUTOINCREMENT,left_clicks INTEGER NOT NULL,right_clicks INTEGER NOT NULL,middle_clicks INTEGER NOT NULL,keys_pressed INTEGER NOT NULL,mouse_moved_cm INTEGER NOT NULL);";
        let query_insert_rows = "INSERT INTO input_logs (left_clicks, right_clicks, middle_clicks, keys_pressed, mouse_moved_cm) VALUES (0, 0, 0, 0, 0);";

        conn.execute(query_create_tables)?;
        conn.execute(query_insert_rows)?;

        let query_create_tables = "CREATE TABLE time_wasted (id INTEGER PRIMARY KEY AUTOINCREMENT,process_name TEXT NOT NULL,seconds_spent INTEGER NOT NULL);";
        conn.execute(query_create_tables)?;
        conn
    } else {
        debug!("Database already exists at: {}", path.display());
        Connection::open(&path)?
    };

    Ok(conn)
}

// it would be pretty cool to have generics stuff and send to db the correct data based on the
// struct that i provided but that seems a lot of pain and quite hard.
// Since i have only two different struct that hold the data, have two different functions seems
// a lot simple and it will make a lot easy to understand the code later.
// TODO: ERROS ARE NOT HANDLED CORRECT I THINK
pub fn update_input_table(logger_data: *const KeyLogger) -> Result<(), Box<dyn std::error::Error>> {
    debug!("Sending Data to database");

    if logger_data.is_null() {
        error!("error, null pointer");
        return Ok(());
    }

    let logger_ref = unsafe { &*logger_data };

    // TODO: THAT LEAD TO ERROS IF I RESTART MY PROGRAM, SHOULD BE FIXED SOON
    let query = format!(
        "UPDATE input_logs SET left_clicks = {}, right_clicks = {}, middle_clicks = {}, keys_pressed = {}, mouse_moved_cm = {};",
        logger_ref.left_clicks,
        logger_ref.right_clicks,
        logger_ref.middle_clicks,
        logger_ref.keys_pressed,
        logger_ref.mouse_moved_cm,
    );

    let conn = get_db_conn()?;
    conn.execute(query)?;
    Ok(())
}

pub fn update_time_table(
    process_map: &HashMap<String, u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    debug!("Sending Data to database");

    let conn = get_db_conn()?;

    for (process, time) in process_map {
        // TODO: CHECK IF ALREADY HAVE A COLUMN WITH THAT NAME.
        let query = format!(
            "INSERT INTO time_wasted (process_name, seconds_spent) VALUES ('{}', {});",
            process.replace("'", "''"), // Handle single quotes in process_name
            time
        );
        conn.execute(query)?;
    }
    Ok(())
}
