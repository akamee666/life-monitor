use crate::common::*;
use crate::keylogger::KeyLogger;

use chrono::Duration;
use chrono::NaiveTime;
use chrono::{Local, Timelike};

use rusqlite::params;
use rusqlite::Connection;
use rusqlite::OpenFlags;
use rusqlite::Result as SqlResult;

use std::fs;
use std::io;
use std::io::Write;

#[cfg(target_os = "linux")]
use crate::platform::linux::common::MouseSettings;
use tracing::*;

#[cfg(target_os = "windows")]
use crate::platform::windows::common::MouseSettings;

pub fn initialize_database(conn: &Connection, requested_gran: Option<u32>) -> SqlResult<u32> {
    // create procs table (it doesn't need any setup)
    conn.execute("CREATE TABLE IF NOT EXISTS procs ( id INTEGER PRIMARY KEY AUTOINCREMENT, w_name TEXT NOT NULL, w_time_foc INTEGER NOT NULL, w_class TEXT NOT NULL);", [])?;

    // create keys table
    let keyst_exist: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='keys');",
        [],
        |row| row.get(0),
    )?;

    // if user didn't specify any granularity level, we create the table with the default (0)
    // and return the gran level of the database for the caller use it
    if !keyst_exist {
        // TODO: magic number
        let gran_level = requested_gran.unwrap_or(0);
        setup_keys_table(conn, gran_level)?;
        return Ok(gran_level);
    }

    let (_, _, current_gran_level) = find_granlevel(conn);

    // keys table exists
    if let Some(new_gran_level) = requested_gran {
        // If we already have keys table and gran was provided,it means the user is changing
        // the level of granularity, so first we need to see how much rows do we have to
        // measure how much levels the user is changing.

        if current_gran_level == new_gran_level {
            info!("Requested granularity level [{new_gran_level}] matches current level [{current_gran_level}]; skipping update.");
            return Ok(current_gran_level);
        }

        // persistent prompt until a valid option is provided.
        println!(
                "\n
                The existing 'keys' table has a granularity '{current_gran_level}', which differs from desired granularity '{new_gran_level}'.\n\
                Would you like to:\n\
                1. (d)rop and recreate the table with the new granularity (all existing data *WILL* be *LOST*)\n\
                2. (r)eorganize the table to match the new granularity (may be imprecise, usually not much)\n"
        );

        let choice = loop {
            print!("Enter your choice [d/r]: ");
            io::stdout().flush().unwrap();

            let mut input = String::new();

            // TODO: Windows subsystem fuck I/O, need to remember it
            if std::io::stdin().read_line(&mut input).is_err() {
                println!("Failed to read your input, please try again.");
                continue;
            }

            let input = input.trim().to_lowercase();
            match input.as_str() {
                "d" | "r" => break input.trim().to_lowercase(),
                _ => {
                    println!(
                        "Invalid option. Please enter 'd' to drop or 'r' to reorganize the table"
                    )
                }
            }
        };

        match choice.as_str() {
            "d" => {
                // Drop the existing table and create a new one
                conn.execute("DROP TABLE keys;", [])?;
                info!("Dropped 'keys' table, recreating with granularity {new_gran_level}.");
                setup_keys_table(conn, new_gran_level)?;
            }
            "r" => {
                // Attempt to reorganize the existing table to fit the new granularity.p
                info!("Reorganizing 'keys' table from {current_gran_level} → {new_gran_level}.");
                reorganize_table(current_gran_level, new_gran_level)?;
            }
            _ => unreachable!(),
        }
        return Ok(new_gran_level);
    }

    info!("Table 'keys' already exists and no granularity change requested.");
    Ok(current_gran_level)
}

/// Creates the `keys` table and populates it with empty rows according to the granularity level.
fn setup_keys_table(conn: &Connection, g_level: u32) -> SqlResult<()> {
    // SQLite does not have a dedicated date/time datatype. Instead, date and time values can stored as any of the following:
    let cq = "CREATE TABLE IF NOT EXISTS keys (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        t_lc INTEGER NOT NULL,
        t_rc INTEGER NOT NULL,
        t_mc INTEGER NOT NULL,
        t_kp INTEGER NOT NULL,
        t_mm INTEGER NOT NULL,
        dpi INTEGER NOT NULL,
        timestamp TEXT NOT NULL
    );";

    let (rows, interval_minutes) = match_gran_level(g_level);
    conn.execute(cq, [])?;

    let today = Local::now().date_naive();
    let mut current_time = today.and_hms_opt(0, 0, 0).unwrap();
    let mut stmt = conn.prepare_cached(
        "INSERT INTO keys (t_lc, t_rc, t_mc, t_kp, t_mm, dpi, timestamp) 
         VALUES (0, 0, 0, 0, 0, 0, ?)",
    )?;
    debug!("today date_naive: {today}");
    for _ in 0..rows {
        let timestamp = current_time.format("%H:%M").to_string();
        stmt.execute([timestamp])?;
        current_time += Duration::minutes(interval_minutes as i64);
    }
    info!("Tables created with granularity level {}!", g_level);
    Ok(())
}

/// Return the matching number of rows and the time interval in minutes from a specific granularity level
fn match_gran_level(gran_level: u32) -> (u32, u32) {
    match gran_level {
        5 => (72, 15),  // 15-minute intervals (72 rows per day)
        4 => (48, 30),  // 30-minute intervals (48 rows per day)
        3 => (24, 60),  // 1-hour intervals (24 rows per day)
        2 => (12, 120), // 2-hour intervals (12 rows per day)
        1 => (6, 240),  // 4-hour intervals (6 rows per day)
        _ => (1, 0),    // 24-hour intervals (1 row per day) // only one entry
    }
}

/// Return the matching number of rows, time interval in minutes and its matching granularity level
/// This *WILL* always make a query to the database, if you don't need the n of rows, use `match_gran_level()`
fn find_granlevel(conn: &Connection) -> (u32, u32, u32) {
    let current_rowsn: u32 = conn
        .query_row("SELECT COUNT(*) FROM keys", [], |row| row.get(0))
        .unwrap_or_else(|err| {
            error!("Failed to determine the current granularity. Table keys might be corrupted");
            panic!("Fatal error: {err}");
        });
    let (rows_n, interval, gran_level) = match current_rowsn {
        72 => (72, 15, 5),  // 15-minute intervals (72 rows)
        48 => (48, 30, 4),  // 30-minute intervals (48 rows)
        24 => (24, 60, 3),  // 1-hour intervals (24 rows)
        12 => (12, 120, 2), // 2-hour intervals (12 rows)
        6 => (6, 240, 1),   // 4-hour intervals (6 rows)
        _ => (1, 1440, 1),  // Default: No granularity (1 row)
    };
    (rows_n, interval, gran_level)
}

fn reorganize_table(current_gran: u32, new_gran: u32) -> SqlResult<()> {
    // determine the number of rows for each granularity level
    let (current_rows_n, _) = match_gran_level(current_gran);
    let (new_rows_n, _) = match_gran_level(new_gran);

    info!(
        "Reorganizing table from: {} rows/day to {} rows/day ",
        current_rows_n, new_rows_n
    );

    if current_gran < new_gran {
        let mut aux_conn = open_con()?;
        // Avoid corrupting the database
        let tx = aux_conn.transaction()?;
        parse_to_higher_gran(&tx, new_gran, new_rows_n)?;
        // we only commit if the parse don't fail
        tx.commit()
    } else {
        let mut aux_conn = open_con()?;
        // Avoid corrupting the database
        let tx = aux_conn.transaction()?;
        let rows_to_merge = current_rows_n / new_rows_n;
        info!("Merging {rows_to_merge} rows");
        parse_to_lower_gran(&tx, new_gran, rows_to_merge)?;
        tx.commit()
    }
}

/// Fetches all rows from the `keys` table as fixed-size integer arrays with an associated timestamp.
/// `N`: The number of integer columns to extract (must be ≤ 6).
/// A vector of tuples where:
/// - The first element is an array `[i32; N]` containing the first `N` columns in order:
///   0: `t_lc`, 1: `t_rc`, 2: `t_mc`, 3: `t_kp`, 4: `t_mm`, 5: `dpi`.
/// - The second element is the timestamp String of the associated row.
/// - If `N` exceeds 6, the query will panic at runtime due to out-of-bounds access.
fn fetch_keys_columns<const N: usize>(conn: &Connection) -> SqlResult<Vec<([i32; N], String)>> {
    let mut stmt = conn.prepare("SELECT t_lc, t_rc, t_mc, t_kp, t_mm, dpi, timestamp FROM keys")?;

    let rows = stmt.query_map([], |row| {
        let mut values = [0; N];
        for (i, item) in values.iter_mut().enumerate().take(N) {
            *item = row.get::<_, i32>(i)?;
        }
        let timestamp: String = row.get(6)?;
        Ok((values, timestamp))
    })?;

    let mut result = Vec::new();
    for row_result in rows {
        result.push(row_result?);
    }
    Ok(result)
}

/// Reorganizes the `keys` table to a higher granularity level.
/// This function performs the following steps:
/// 1. Computes the total values of the five key columns (`t_lc`, `t_rc`, `t_mc`, `t_kp`, `t_mm`)
///    from the existing table to preserve cumulative data.
/// 2. Drops and recreates the `keys` table with the new granularity schema requested by the user.
/// 3. Precomputes average values for each key column based on the old totals and the desired number of new rows.
/// 4. Updates each row with the precomputed average values.
/// 5. Commits the transaction, ensuring that either all changes are applied or none if an error occurs.
///
/// - Accuracy may be imperfect when changing to a higher granularity because data is averaged and redistributed.
/// - All operations are performed in a single transaction to prevent partial updates or corruption.
fn parse_to_higher_gran(conn: &Connection, new_gran: u32, new_rows_n: u32) -> SqlResult<()> {
    warn!("Cannot reorganize with full accuracy when changing to a higher granularity; summing and redistributingdata...");

    let rows: Vec<([i32; 5], String)> = fetch_keys_columns::<5>(conn)?;

    let mut old_totals = [0; 5];
    for (rows, _) in rows.iter() {
        for (total, val) in old_totals.iter_mut().zip(rows.iter()) {
            *total += val;
        }
    }

    // recreate table
    conn.execute("DROP TABLE IF EXISTS keys", [])?;
    setup_keys_table(conn, new_gran)?;

    // this will be sorted according to the SELECT query
    let averages: Vec<i32> = old_totals.iter().map(|&x| x / new_rows_n as i32).collect();
    for i in 1..=new_rows_n {
        conn.execute(
            "
        UPDATE keys
        SET t_lc = ?, t_rc = ?, t_mc = ?, t_kp = ?, t_mm = ?, dpi = ?
        WHERE id = ?",
            params![
                averages[0],
                averages[1],
                averages[2],
                averages[3],
                averages[4],
                800,
                i
            ],
        )?;
    }

    let rows: Vec<([i32; 5], String)> = fetch_keys_columns::<5>(conn)?;
    let mut new_totals = [0; 5];
    for (rows, _) in rows.iter() {
        for (total, val) in new_totals.iter_mut().zip(rows.iter()) {
            *total += val;
        }
    }

    info!("Table reorganization complete.");
    println!("Reorganization accuracy:");
    let labels = ["t_lc", "t_rc", "t_mc", "t_kp", "t_mm"];
    labels
        .iter()
        .zip(new_totals.iter())
        .zip(old_totals.iter())
        .for_each(|((label, &new_val), &old_val)| {
            if old_val == 0 {
                println!("  {}: N/A (original total is zero)", label);
            } else {
                let accuracy = (new_val as f64 / old_val as f64) * 100.0;
                println!("  {}: {:.2}%", label, accuracy);
            }
        });

    Ok(())
}

fn parse_to_lower_gran(conn: &Connection, new_gran: u32, rows_to_merge: u32) -> SqlResult<()> {
    // go to parse_to_higher_gran to see why we open another connection here
    info!(
        "Reorganizing keys table to a lower granularity level. Each new row will be an aggregation of {} original rows.",
        rows_to_merge
    );

    // TODO: This amount of magic numbers is bothering me, i should create a struct for better readiblity
    let rows: Vec<([i32; 6], String)> = fetch_keys_columns(conn)?;

    let mut aggregated_rows = Vec::new();
    for chunk in rows.chunks(rows_to_merge as usize) {
        if chunk.is_empty() {
            continue;
        }

        let timestamp = chunk.first().unwrap().1.clone();
        let mut totals = [0; 6]; // [t_lc, t_rc, t_mc, t_kp, t_mm, dpi]

        // sum each column in the chunk into `totals`
        // totals[i] += values[i] for each row in the current chunk
        for (values, _) in chunk {
            for (total, val) in totals.iter_mut().zip(values.iter()) {
                *total += val;
            }
        }
        let dpi = totals[5]; // dpi is the same in all the rows

        aggregated_rows.push((
            totals[0], totals[1], totals[2], totals[3], totals[4], dpi, timestamp,
        ));
    }

    conn.execute("DROP TABLE keys;", [])?;
    setup_keys_table(conn, new_gran)?;

    let q = "INSERT INTO keys (t_lc, t_rc, t_mc, t_kp, t_mm, dpi, timestamp) 
         VALUES (0, 0, 0, 0, 0, 0, ?)";

    for row in aggregated_rows {
        conn.execute(q, params![row.0, row.1, row.2, row.3, row.4, row.5, row.6])?;
    }

    info!(
        "Successfully reorganized and updated data to new granularity level {}.",
        new_gran
    );

    Ok(())
}

pub fn clear_database() -> std::io::Result<()> {
    let mut db_path = program_data_dir()?;
    db_path.push("data.db");

    if let Err(err) = fs::remove_file(db_path.clone()) {
        match err.kind() {
            io::ErrorKind::NotFound => {
                error!(
                    "Skipping cleanup: database file not found at '{}'",
                    db_path.display()
                );
                return Ok(());
            }
            _ => return Err(err),
        }
    }

    info!(
        "Database at '{}' was succesfully deleted!",
        db_path.display()
    );
    Ok(())
}

/// The function bucket_start takes a specific time (like "10:55") and an interval (like 30 minutes) and figures out the starting time of the "bucket" or "slot" it belongs to.
fn find_bucket(current_time: &str, interval_minutes: u32) -> Option<String> {
    let time = match NaiveTime::parse_from_str(current_time, "%H:%M") {
        Ok(t) => t,
        Err(err) => {
            // Yes this is fatal ble :x
            error!("Failed to parse time when attempting to find the nearest hour in database");
            panic!("Fatal error: {err}");
        }
    };

    // lets work with minutes instead, hours are confusing :p
    let total_minutes = time.hour() as i64 * 60 + time.minute() as i64;
    // this will return the amount of minutes we are past the start of the current bucket
    let offset_interval = total_minutes % interval_minutes as i64;
    // This will floor the current time in minutes by removing the reminder offset so it match the interval
    let floored = total_minutes - offset_interval;

    // convert to secs to match fn signature
    let floored_secs = (floored * 60) as u32;
    let time_bucket = NaiveTime::from_num_seconds_from_midnight_opt(floored_secs, 0)?;
    let time_bucket_formated = time_bucket.format("%H:%M").to_string();
    info!("Found the bucket we need to update: {time_bucket_formated}");
    Some(time_bucket_formated)
}

pub fn update_keyst(conn: &Connection, logger_data: &KeyLogger) -> SqlResult<()> {
    let now = Local::now();
    let now = format!("{:02}:{:02}", now.hour(), now.minute());

    let (_, interval, _) = find_granlevel(conn);
    let Some(bucket) = find_bucket(&now, interval) else {
        error!("No valid bucket found for time {}", now);
        return Ok(());
    };

    let q = "
            UPDATE keys
            SET t_lc = ?,
                t_rc = ?,
                t_mc = ?,
                t_kp = ?,
                t_mm = ?,
                dpi = ?
            WHERE timestamp = ?;
        ";

    let affected_rows = conn.execute(
        q,
        params![
            logger_data.t_lc,
            logger_data.t_rc,
            logger_data.t_mc,
            logger_data.t_kp,
            // TODO: this may fuck up something
            logger_data.t_mm.floor(),
            logger_data.mouse_settings.dpi,
            bucket,
        ],
    )?;

    match affected_rows {
        1 => debug!(
            "Updated bucket '{}': LC=[{}], RC=[{}], MC=[{}], KP=[{}], MM=[{}], DPI=[{}]",
            bucket,
            logger_data.t_lc,
            logger_data.t_rc,
            logger_data.t_mc,
            logger_data.t_kp,
            logger_data.t_mm,
            logger_data.mouse_settings.dpi
        ),
        0 => warn!("No row matched timestamp '{}'", bucket),
        _ => error!(
            "Unexpected: {} rows updated for '{}'",
            affected_rows, bucket
        ),
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
        SELECT w_name,w_time_foc, w_class
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
            let query_update = "UPDATE procs SET w_time_foc = ?, w_class = ? WHERE w_name = ?;";
            conn.execute(
                query_update,
                params![process.w_time, process.w_class, process.w_name],
            )?;
        } else {
            //debug!("w_name: {} does not exists!", &process.w_name);
            let query_insert =
                "INSERT INTO procs (w_name, w_time_foc, w_class) VALUES (?, ?, ?, ?);";
            conn.execute(
                query_insert,
                params![process.w_name, process.w_time, process.w_class],
            )?;
        }
    }
    Ok(())
}

pub fn open_con() -> SqlResult<Connection> {
    let mut db_path = program_data_dir().unwrap_or_else(|err| {
        error!("Failed to resolve database file path");
        panic!("Fatal error: {err}");
    });
    db_path.push("data.db");

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;
    Ok(conn)
}

#[cfg(test)]
#[allow(dead_code)]
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

    // #[test]
    // fn test_exact_match() -> SqlResult<()> {
    //     let conn = setup_database(&["08:00", "12:00", "16:00", "20:00"])?;
    //     let nearest = find_nearest_time(&conn, "12:00")?;
    //     assert_eq!(nearest, Some("12:00".to_string()));
    //     Ok(())
    // }
    //
    // #[test]
    // fn test_closer_to_future_timestamp() -> SqlResult<()> {
    //     let conn = setup_database(&["08:00", "12:00", "16:00", "20:00"])?;
    //     let nearest = find_nearest_time(&conn, "11:55")?;
    //     assert_eq!(nearest, Some("12:00".to_string()));
    //     Ok(())
    // }
    //
    // #[test]
    // fn test_closer_to_past_timestamp() -> SqlResult<()> {
    //     let conn = setup_database(&["08:00", "12:00", "16:00", "20:00"])?;
    //     let nearest = find_nearest_time(&conn, "13:05")?;
    //     assert_eq!(nearest, Some("12:00".to_string()));
    //     Ok(())
    // }
    //
    // #[test]
    // fn test_15_minute_intervals() -> SqlResult<()> {
    //     let conn = setup_database(&[
    //         "00:00", "00:15", "00:30", "00:45", "01:00", "01:15", "01:30", "01:45",
    //     ])?;
    //     let nearest = find_nearest_time(&conn, "00:10")?;
    //     assert_eq!(nearest, Some("00:15".to_string()));
    //
    //     let nearest = find_nearest_time(&conn, "00:25")?;
    //     assert_eq!(nearest, Some("00:30".to_string()));
    //
    //     let nearest = find_nearest_time(&conn, "01:50")?;
    //     assert_eq!(nearest, Some("01:45".to_string()));
    //
    //     Ok(())
    // }
    //
    // #[test]
    // fn test_minute_intervals() -> SqlResult<()> {
    //     let conn = setup_database(&["12:00", "12:01", "12:02", "12:03", "12:04", "12:05"])?;
    //     let nearest = find_nearest_time(&conn, "12:02")?;
    //     assert_eq!(nearest, Some("12:02".to_string()));
    //
    //     let nearest = find_nearest_time(&conn, "12:02:30")?;
    //     assert_eq!(nearest, Some("12:03".to_string()));
    //
    //     let nearest = find_nearest_time(&conn, "12:00:30")?;
    //     assert_eq!(nearest, Some("12:01".to_string()));
    //
    //     Ok(())
    // }
}
