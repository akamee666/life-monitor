use crate::keylogger::KeyLogger;
use rusqlite::{params, Connection};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};
use tracing::{debug, info, warn};

// it would be pretty cool to have generics stuff and send to db the correct data based on the
// struct that i provided but that seems a lot of pain and quite hard.
// Since i have only two different struct that hold the data, have two different functions seems
// a lot simple and it will make a lot easy to understand the code later.
pub fn send_to_input_table(logger_data: &KeyLogger) -> Result<(), Box<dyn std::error::Error>> {
    warn!("Sending data to inputs table");
    let conn = get_db_conn()?;

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
pub fn get_input_data() -> Result<KeyLogger, Box<dyn std::error::Error>> {
    let query = "
    SELECT left_clicks, right_clicks, middle_clicks, keys_pressed, mouse_moved_cm 
    FROM input_logs 
    LIMIT 1;
    ";

    let conn = get_db_conn()?;
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
        })
    })?;

    Ok(row)
}

pub fn get_process_data() -> Result<HashMap<String, u64>, Box<dyn std::error::Error>> {
    let conn = get_db_conn()?;

    let query = "
        SELECT process_name, seconds_spent
        FROM time_wasted;
    ";

    let mut stmt = conn.prepare(query)?;

    let mut process_map = HashMap::new();

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?, // process_name
            row.get::<_, u64>(1)?,    // seconds_spent
        ))
    })?;

    for row in rows {
        let (process_name, seconds_spent) = row?;
        process_map.insert(process_name, seconds_spent);
    }

    Ok(process_map)
}

pub fn send_to_process_table(
    process_map: &HashMap<String, u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    warn!("Sending Data to processes table");
    let conn = get_db_conn()?;

    // Iterate over each process and time in the map
    for (process, time) in process_map {
        // Check if the process already exists in the table
        let query_check = "SELECT 1 FROM time_wasted WHERE process_name = ? LIMIT 1;";
        let mut stmt_check = conn.prepare(query_check)?;
        let exists = stmt_check.exists(params![process])?;

        if exists {
            debug!(
                "Already have this program in processes table, updating values for {}",
                process
            );
            let query_update = "UPDATE time_wasted SET seconds_spent = ? WHERE process_name = ?;";
            conn.execute(query_update, params![time, process])?;
        } else {
            let query_insert =
                "INSERT INTO time_wasted (process_name, seconds_spent) VALUES (?, ?);";
            conn.execute(query_insert, params![process, time])?;
        }
    }
    Ok(())
}

fn get_db_conn() -> Result<Connection, Box<dyn std::error::Error>> {
    info!("Sarting db connection");

    let local_app_data = env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\Temp".into());
    let mut path = PathBuf::from(&local_app_data);
    path.push("akame_monitor");
    path.push("tracked_data.db");

    if let Some(parent_dir) = path.parent() {
        fs::create_dir_all(parent_dir)?;
    }

    let conn = if !Path::new(&path).exists() {
        debug!("Database created at: {}", path.display());
        let conn = Connection::open(&path)?;

        // Create tables
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

        let query_insert_initial_rows = "
            INSERT INTO input_logs (left_clicks, right_clicks, middle_clicks, keys_pressed, mouse_moved_cm)
            VALUES (0, 0, 0, 0, 0);
        ";

        conn.execute(query_insert_initial_rows, [])?;

        let query_create_time_wasted_table = "
            CREATE TABLE time_wasted (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                process_name TEXT NOT NULL,
                seconds_spent INTEGER NOT NULL
            );
        ";
        conn.execute(query_create_time_wasted_table, [])?;

        conn
    } else {
        Connection::open(&path)?
    };

    Ok(conn)
}
