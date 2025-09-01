use crate::common::*;
use crate::keylogger::KeyLogger;

use chrono::NaiveTime;
use chrono::{Duration, NaiveDate};
use chrono::{Local, Timelike};

use rusqlite::params;
use rusqlite::Connection;
use rusqlite::OpenFlags;
use rusqlite::Result as SqlResult;

use std::fs;
use std::io;
use std::io::Write;

use tracing::*;

#[cfg(target_os = "linux")]
use crate::platform::linux::common::MouseSettings;

#[cfg(target_os = "windows")]
use crate::platform::windows::common::MouseSettings;

pub fn initialize_database(conn: &Connection, k_gran: Option<u32>) -> SqlResult<()> {
    let pq = "CREATE TABLE IF NOT EXISTS procs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        w_name TEXT NOT NULL,
        w_time INTEGER NOT NULL,
        w_class TEXT NOT NULL
    );";

    // Create the 'procs' table.
    conn.execute(pq, [])?;

    let mut stmt = conn.prepare(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='keys');",
    )?;
    let table_exists: bool = stmt.query_row([], |row| row.get(0))?;

    if table_exists {
        if let Some(new_gran) = k_gran {
            // If we already have keys table and gran was provided,it means the user is changing
            // the level of granularity, so first we need to see how much rows do we have to
            // measure how much levels the user is changing.
            let current_gran = match conn
                .query_row("SELECT COUNT(*) FROM keys", [], |row| row.get(0))
            {
                Ok(count) => match count {
                    72 => 5, // 15-minute intervals (72 rows)
                    48 => 4, // 30-minute intervals (48 rows)
                    24 => 3, // 1-hour intervals (24 rows)
                    12 => 2, // 2-hour intervals (12 rows)
                    6 => 1,  // 4-hour intervals (6 rows)
                    _ => 0,  // Default: No granularity (1 row)
                },
                Err(e) => {
                    error!("Failed to determine the current granularity. Ensure the keys table was correctly initialized.");
                    panic!("{e}");
                }
            };

            if current_gran == new_gran {
                info!("The requested level is the same. No changes needed.");
                return Ok(());
            }

            // Persistent prompt until a valid option is provided.
            println!();
            println!(
                    "The existing 'keys' table has a granularity level of {current_gran}.\n\
                    This does not match the new desired granularity level of {new_gran}.\n\
                    Would you like to:\n\
                    1. (d)rop the existing table and create a new one with the new granularity (this will erase all previous data)\n\
                    2. (r)eorganize the existing table to fit the new granularity (this may not have 100% accuracy)\n"
                );

            let choice = loop {
                print!("Enter your choice [D/r]: ");
                io::stdout().flush().unwrap();

                let mut input = String::new();
                if let Err(e) = std::io::stdin().read_line(&mut input) {
                    warn!("Failed to read input: {}", e);
                    println!("Please try again.");
                    continue;
                }

                let input = input.trim().to_lowercase();
                match input.as_str() {
                    "d" => break "d",
                    "r" => break "r",
                    _ => println!("Invalid option. Please enter 'd' to drop or 'r' to reorganize."),
                }
            };

            match choice {
                "d" => {
                    // Drop the existing table and create a new one
                    conn.execute("DROP TABLE keys;", [])?;
                    info!("User choose to drop existent keys table. Table was dropped, creating a new one with granularity: {new_gran}.");
                    create_keys_table(conn, new_gran)?;
                }
                "r" => {
                    // Attempt to reorganize the existing table to fit the new granularity.
                    reorganize_table(conn, current_gran, new_gran)?;
                }
                _ => unreachable!(),
            }
        } else {
            info!("Table keys already exists, and no new granularity level was specified. No changes needed.");
        }
        return Ok(());
    }
    info!("Table keys does not exist. Creating new table with provided granularity or the default value.");
    create_keys_table(conn, k_gran.unwrap_or(0))
}

fn create_keys_table(c: &Connection, g_level: u32) -> SqlResult<()> {
    // SQLite does not have a dedicated date/time datatype. Instead, date and time values can stored as any of the following:
    let kq = "CREATE TABLE IF NOT EXISTS keys (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        t_lc INTEGER NOT NULL,
        t_rc INTEGER NOT NULL,
        t_mc INTEGER NOT NULL,
        t_kp INTEGER NOT NULL,
        t_mm INTEGER NOT NULL,
        dpi INTEGER NOT NULL,
        timestamp TEXT NOT NULL
    );";

    // Define intervals and row counts for each g_level level.
    let (rows, interval_minutes) = match g_level {
        5 => (72, 15),  // 15-minute intervals (72 rows per day)
        4 => (48, 30),  // 30-minute intervals (48 rows per day)
        3 => (24, 60),  // 1-hour intervals (24 rows per day)
        2 => (12, 120), // 2-hour intervals (12 rows per day)
        1 => (6, 240),  // 4-hour intervals (6 rows per day)
        _ => {
            c.execute(kq, [])?;
            insert_rows(c, 1, 0)?;
            info!("Tables created with default configuration!");
            return Ok(());
        }
    };
    c.execute(kq, [])?;
    insert_rows(c, rows, interval_minutes)?;
    info!("Tables created with granularity level {}!", g_level);
    Ok(())
}

fn reorganize_table(conn: &Connection, current_gran: u32, new_gran: u32) -> SqlResult<()> {
    //Determine the number of rows for each granularity level
    let current_rows_n = match current_gran {
        5 => 72, // 15-minute intervals
        4 => 48, // 30-minute intervals
        3 => 24, // 1-hour intervals
        2 => 12, // 2-hour intervals
        1 => 6,  // 4-hour intervals
        _ => 1,  // Default (raw total)
    };

    let new_rows_n = match new_gran {
        5 => 72, // 15-minute intervals
        4 => 48, // 30-minute intervals
        3 => 24, // 1-hour intervals
        2 => 12, // 2-hour intervals
        1 => 6,  // 4-hour intervals
        _ => 1,  // Default (raw total)
    };

    info!(
        "Reorganizing table from: {} rows/day to {} rows/day ",
        current_rows_n, new_rows_n
    );

    if current_gran < new_gran {
        parse_to_higher_gran(conn, new_gran, new_rows_n)
    } else {
        let rows_to_merge = current_rows_n / new_rows_n;
        parse_to_lower_gran(conn, new_gran, rows_to_merge)
    }
}

fn insert_rows(conn: &Connection, rows: u32, interval_minutes: i64) -> SqlResult<()> {
    // This might panic yayy :)
    let today = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let mut current_time = today.and_hms_opt(0, 0, 0).unwrap();

    // Use interval_minutes directly
    for _ in 0..rows {
        let timestamp = current_time.format("%H:%M").to_string();
        conn.execute(
            "INSERT INTO keys (t_lc, t_rc, t_mc, t_kp, t_mm, dpi, timestamp) 
                 VALUES (0, 0, 0, 0, 0, 0, ?)",
            [timestamp],
        )?;

        // Increment time by the correct number of minutes
        current_time += Duration::minutes(interval_minutes);
    }
    Ok(())
}

fn parse_to_higher_gran(conn: &Connection, new_gran: u32, rows: i32) -> SqlResult<()> {
    warn!("Cannot reorganize with full accuracy when changing to a higher granularity. Summing data and redistributing...");
    // Fetch all the existing data, sum it and divide through the table.
    let mut total_lc = 0;
    let mut total_rc = 0;
    let mut total_mc = 0;
    let mut total_kp = 0;
    let mut total_mm = 0;
    let mut stmt = conn.prepare("SELECT t_lc, t_rc, t_mc, t_kp, t_mm, dpi, timestamp FROM keys")?;
    let mut rows_result = stmt.query([])?;
    while let Some(row) = rows_result.next()? {
        total_lc += row.get::<_, i32>(0)?;
        total_rc += row.get::<_, i32>(1)?;
        total_mc += row.get::<_, i32>(2)?;
        total_kp += row.get::<_, i32>(3)?;
        total_mm += row.get::<_, i32>(4)?;
    }
    drop(rows_result);

    // Drop the old table
    conn.execute("DROP TABLE IF EXISTS keys", [])?;
    create_keys_table(conn, new_gran)?;

    // If we are increasing granularity, we need to redistribute the summed data evenly
    let num_new_rows = rows;
    let avg_lc = total_lc / num_new_rows;
    let avg_rc = total_rc / num_new_rows;
    let avg_mc = total_mc / num_new_rows;
    let avg_kp = total_kp / num_new_rows;
    let avg_mm = total_mm / num_new_rows;

    let mut stmt = conn.prepare(
        "
        UPDATE keys
        SET t_lc = ?,
            t_rc = ?,
            t_mc = ?,
            t_kp = ?,
            t_mm = ?,
            dpi = ?
        WHERE id = ?",
    )?;

    for i in 1..=num_new_rows {
        stmt.execute(params![avg_lc, avg_rc, avg_mc, avg_kp, avg_mm, 800, i])?;
    }

    // Display accuracy percentage.
    let mut total_lc_new = 0;
    let mut total_rc_new = 0;
    let mut total_mc_new = 0;
    let mut total_kp_new = 0;
    let mut total_mm_new = 0;
    stmt = conn.prepare("SELECT t_lc, t_rc, t_mc, t_kp, t_mm FROM keys")?;
    let mut rows_result_new = stmt.query([])?;
    while let Some(row) = rows_result_new.next()? {
        total_lc_new += row.get::<_, i32>(0)?;
        total_rc_new += row.get::<_, i32>(1)?;
        total_mc_new += row.get::<_, i32>(2)?;
        total_kp_new += row.get::<_, i32>(3)?;
        total_mm_new += row.get::<_, i32>(4)?;
    }
    drop(rows_result_new);

    let accuracy_lc = (total_lc_new as f64 / total_lc as f64) * 100.0;
    let accuracy_rc = (total_rc_new as f64 / total_rc as f64) * 100.0;
    let accuracy_mc = (total_mc_new as f64 / total_mc as f64) * 100.0;
    let accuracy_kp = (total_kp_new as f64 / total_kp as f64) * 100.0;
    let accuracy_mm = (total_mm_new as f64 / total_mm as f64) * 100.0;

    println!(
        "Reorganization accuracy:\n\
         t_lc: {:.2}%\n\
         t_rc: {:.2}%\n\
         t_mc: {:.2}%\n\
         t_kp: {:.2}%\n\
         t_mm: {:.2}%",
        accuracy_lc, accuracy_rc, accuracy_mc, accuracy_kp, accuracy_mm
    );

    info!("Table reorganization complete.");
    Ok(())
}

fn parse_to_lower_gran(conn: &Connection, new_gran: u32, rows_to_merge: i32) -> SqlResult<()> {
    let mut stmt = conn.prepare("SELECT t_lc, t_rc, t_mc, t_kp, t_mm, dpi, timestamp FROM keys")?;

    // Calculate how many old rows will be merged into a single new row
    info!(
        "Reorganizing 'keys' table to a lower granularity level. \
        Each new row will be an aggregation of {} original rows.",
        rows_to_merge
    );

    // Get all values from each row inside a vector.
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i32>(0)?,    // t_lc
                row.get::<_, i32>(1)?,    // t_rc
                row.get::<_, i32>(2)?,    // t_mc
                row.get::<_, i32>(3)?,    // t_kp
                row.get::<_, i32>(4)?,    // t_mm
                row.get::<_, i32>(5)?,    // dpi
                row.get::<_, String>(6)?, // timestamp
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // Step 2: Aggregate data based on the new granularity
    let mut aggregated_rows = Vec::new();
    for chunk in rows.chunks(rows_to_merge as usize) {
        let mut t_lc = 0;
        let mut t_rc = 0;
        let mut t_mc = 0;
        let mut t_kp = 0;
        let mut t_mm = 0;
        let mut dpi = 0;
        let timestamp = if let Some(first) = chunk.first() {
            first.6.clone() // Take timestamp from the first row in the chunk
        } else {
            continue;
        };

        for row in chunk {
            t_lc += row.0;
            t_rc += row.1;
            t_mc += row.2;
            t_kp += row.3;
            t_mm += row.4;
            dpi = row.5; // Keep the most recent dpi value because it does not need to change.
        }

        aggregated_rows.push((t_lc, t_rc, t_mc, t_kp, t_mm, dpi, timestamp));
    }

    // Step 3: Drop the existing table and create a new one with the desired granularity
    conn.execute("DROP TABLE keys;", [])?;
    create_keys_table(conn, new_gran)?;

    // Step 4: Update the existing data with the aggregated values
    let update_query = "
    UPDATE keys
    SET t_lc = ?1, t_rc = ?2, t_mc = ?3, t_kp = ?4, t_mm = ?5, dpi = ?6
    WHERE timestamp = ?7;
";
    let mut update_stmt = conn.prepare(update_query)?;

    for row in aggregated_rows {
        update_stmt.execute(params![row.0, row.1, row.2, row.3, row.4, row.5, row.6])?;
    }

    info!(
        "Successfully reorganized and updated data to new granularity level {}.",
        new_gran
    );

    Ok(())
}

pub fn clean_database() -> std::io::Result<()> {
    let mut path = find_path()?;
    path.push("data.db");
    fs::remove_file(path.clone())?;
    info!("Database deleted at {} with success!", path.display());
    Ok(())
}

fn find_nearest_time(conn: &Connection, current_hour: &str) -> SqlResult<Option<String>> {
    // Query to find the nearest timestamp less than or equal to the current time
    let query_find_prev = "
        SELECT timestamp FROM keys
        WHERE timestamp <= ?
        ORDER BY timestamp DESC
        LIMIT 1;
    ";

    // Query to find the nearest timestamp greater than or equal to the current time
    let query_find_next = "
        SELECT timestamp FROM keys
        WHERE timestamp >= ?
        ORDER BY timestamp ASC
        LIMIT 1;
    ";

    // Prepare and execute the queries
    let mut stmt_prev = conn.prepare(query_find_prev)?;
    let mut stmt_next = conn.prepare(query_find_next)?;

    let prev_timestamp: Option<String> = stmt_prev.query_row([current_hour], |row| row.get(0)).ok();

    let next_timestamp: Option<String> = stmt_next.query_row([current_hour], |row| row.get(0)).ok();

    // Convert current time to NaiveTime
    let current_time = NaiveTime::parse_from_str(current_hour, "%H:%M").unwrap();

    // Parse the timestamps to NaiveTime and find the closest one
    let prev_diff = prev_timestamp.as_ref().map(|ts| {
        let prev_time = NaiveTime::parse_from_str(ts, "%H:%M").unwrap();
        (current_time - prev_time).num_seconds().abs()
    });

    let next_diff = next_timestamp.as_ref().map(|ts| {
        let next_time = NaiveTime::parse_from_str(ts, "%H:%M").unwrap();
        (next_time - current_time).num_seconds().abs()
    });

    // Determine which timestamp is closer
    // i might change this part this is a little rusty and hard to read
    let nearest_timestamp = match (prev_diff, next_diff) {
        (Some(prev_diff), Some(next_diff)) => {
            if prev_diff <= next_diff {
                prev_timestamp
            } else {
                next_timestamp
            }
        }
        (Some(_), None) => prev_timestamp,
        (None, Some(_)) => next_timestamp,
        (None, None) => None,
    };

    Ok(nearest_timestamp)
}

pub fn update_keyst(conn: &Connection, logger_data: &KeyLogger) -> SqlResult<()> {
    // Get the current hour and minute
    let current_time = Local::now();
    let current_hour = format!("{:02}:{:02}", current_time.hour(), current_time.minute());

    debug!(
        "Updating keys table for the nearest interval to current time: {}",
        current_hour
    );

    if let Some(timestamp) = find_nearest_time(conn, &current_hour)? {
        debug!("Found nearest timestamp: {}", timestamp);

        // Prepare the update query to update the row with the found timestamp
        let query_update = "
            UPDATE keys
            SET t_lc = ?,
                t_rc = ?,
                t_mc = ?,
                t_kp = ?,
                t_mm = ?,
                dpi = ?
            WHERE timestamp = ?;
        ";

        // Execute the query to update the correct row
        let affected_rows = conn.execute(
            query_update,
            params![
                logger_data.t_lc,
                logger_data.t_rc,
                logger_data.t_mc,
                logger_data.t_kp,
                // this may fuck up something
                logger_data.t_mm.floor(),
                logger_data.mouse_settings.dpi,
                timestamp,
            ],
        )?;

        if affected_rows > 0 {
            debug!(
                "Successfully updated row for timestamp '{}'. Changes: LC = {}, RC = {}, MC = {}, KP = {}, MM = {}, DPI = {}",
                timestamp,
                logger_data.t_lc,
                logger_data.t_rc,
                logger_data.t_mc,
                logger_data.t_kp,
                logger_data.t_mm,
                logger_data.mouse_settings.dpi
            );
        } else {
            warn!("Update failed for timestamp '{}'.", timestamp);
        }
    } else {
        warn!(
            "No suitable timestamp found for current time: {}. Ensure the keys table is correctly initialize, if you did nothing and receive this message please create an issue in github.",
            current_hour
        );
    }

    Ok(())
}

// This might not work anymore.
// And i guess i should create a ticker that will reset keylogger values at each database interval.
pub fn get_keyst(conn: &Connection) -> SqlResult<KeyLogger> {
    let query = "
    SELECT t_lc, t_rc, t_mc, t_kp, t_mm,dpi
    FROM keys
    LIMIT 1;
    ";

    let mut stmt = conn.prepare(query)?;

    // Assuming there is only one row
    let row = stmt.query_row([], |row| {
        let k = KeyLogger {
            t_lc: row.get(0)?,
            t_rc: row.get(1)?,
            t_mc: row.get(2)?,
            t_kp: row.get(3)?,
            t_mm: row.get(4)?,
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
        SELECT w_name,w_time, w_class
        FROM procs;
    ";

    let mut stmt = conn.prepare(query)?;

    let process_vec = stmt
        .query_map([], |row| {
            Ok(ProcessInfo {
                w_name: row.get(0)?,
                w_time: row.get(1)?,
                w_class: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<ProcessInfo>, _>>()?;

    Ok(process_vec)
}

// Only borrowing and only reading the data, &[ProcessInfo] is better in that case.
pub fn update_proct(conn: &Connection, process_vec: &[ProcessInfo]) -> SqlResult<()> {
    for process in process_vec {
        let check_q = "SELECT 1 FROM procs WHERE w_name = ? LIMIT 1;";
        let mut stmt_check = conn.prepare(check_q)?;
        let exists = stmt_check.exists(params![&process.w_name])?;

        if exists {
            //debug!("w_name: {} exists!", &process.w_name);
            let query_update = "UPDATE procs SET w_time = ?, w_class = ? WHERE w_name = ?;";
            conn.execute(
                query_update,
                params![process.w_time, process.w_class, process.w_name],
            )?;
        } else {
            //debug!("w_name: {} does not exists!", &process.w_name);
            let query_insert = "INSERT INTO procs (w_name,w_time, w_class) VALUES (?, ?, ?, ?);";
            conn.execute(
                query_insert,
                params![process.w_name, process.w_time, process.w_class],
            )?;
        }
    }
    Ok(())
}

pub fn open_con() -> SqlResult<Connection> {
    let path = match find_path() {
        Ok(path) => path.join("data.db"),
        Err(e) => {
            error!("Could not find path for database file.\n Error: {e}");
            panic!();
        }
    };
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Result as SqlResult;

    fn setup_database(data: &[&str]) -> SqlResult<Connection> {
        let conn = Connection::open_in_memory()?;
        insert_data(&conn, data)?;
        Ok(conn)
    }

    fn insert_data(conn: &Connection, data: &[&str]) -> SqlResult<()> {
        conn.execute(
            "CREATE TABLE keys (id INTEGER PRIMARY KEY, timestamp TEXT)",
            [],
        )?;
        let mut stmt = conn.prepare("INSERT INTO keys (timestamp) VALUES (?)")?;
        for &timestamp in data {
            stmt.execute([timestamp])?;
        }
        Ok(())
    }

    #[test]
    fn test_exact_match() -> SqlResult<()> {
        let conn = setup_database(&["08:00", "12:00", "16:00", "20:00"])?;
        let nearest = find_nearest_time(&conn, "12:00")?;
        assert_eq!(nearest, Some("12:00".to_string()));
        Ok(())
    }

    #[test]
    fn test_closer_to_future_timestamp() -> SqlResult<()> {
        let conn = setup_database(&["08:00", "12:00", "16:00", "20:00"])?;
        let nearest = find_nearest_time(&conn, "11:55")?;
        assert_eq!(nearest, Some("12:00".to_string()));
        Ok(())
    }

    #[test]
    fn test_closer_to_past_timestamp() -> SqlResult<()> {
        let conn = setup_database(&["08:00", "12:00", "16:00", "20:00"])?;
        let nearest = find_nearest_time(&conn, "13:05")?;
        assert_eq!(nearest, Some("12:00".to_string()));
        Ok(())
    }

    #[test]
    fn test_15_minute_intervals() -> SqlResult<()> {
        let conn = setup_database(&[
            "00:00", "00:15", "00:30", "00:45", "01:00", "01:15", "01:30", "01:45",
        ])?;
        let nearest = find_nearest_time(&conn, "00:10")?;
        assert_eq!(nearest, Some("00:15".to_string()));

        let nearest = find_nearest_time(&conn, "00:25")?;
        assert_eq!(nearest, Some("00:30".to_string()));

        let nearest = find_nearest_time(&conn, "01:50")?;
        assert_eq!(nearest, Some("01:45".to_string()));

        Ok(())
    }

    #[test]
    fn test_minute_intervals() -> SqlResult<()> {
        let conn = setup_database(&["12:00", "12:01", "12:02", "12:03", "12:04", "12:05"])?;
        let nearest = find_nearest_time(&conn, "12:02")?;
        assert_eq!(nearest, Some("12:02".to_string()));

        let nearest = find_nearest_time(&conn, "12:02:30")?;
        assert_eq!(nearest, Some("12:03".to_string()));

        let nearest = find_nearest_time(&conn, "12:00:30")?;
        assert_eq!(nearest, Some("12:01".to_string()));

        Ok(())
    }
}
