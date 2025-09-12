use crate::common::*;

use chrono::Duration;
use chrono::NaiveTime;
use chrono::{Local, Timelike};

use rusqlite::params;
use rusqlite::Connection;
use rusqlite::OpenFlags;
use rusqlite::Result as SqlResult;

use anyhow::{Context, Result};

use std::fs;
use std::io;
use std::io::Write;

use tracing::*;

#[cfg(target_os = "windows")]
use crate::platform::windows::common::MouseSettings;

pub fn setup_database(conn: &Connection, requested_gran: Option<u32>) -> Result<u32> {
    // create procs table (it doesn't need any setup)
    conn.execute("CREATE TABLE IF NOT EXISTS procs ( id INTEGER PRIMARY KEY AUTOINCREMENT, w_name TEXT NOT NULL, w_time_foc INTEGER NOT NULL, w_class TEXT NOT NULL);", []).with_context(|| "Failed to initialize procs table")?;

    // create keys table
    let keyst_exist: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='keys');",
            [],
            |row| row.get(0),
        )
        .with_context(|| "Failed to check if keys table exists")?;

    // if user didn't specify any granularity level, we create the table with the default (0)
    // and return the gran level of the database for the caller use it
    if !keyst_exist {
        // TODO: magic number
        let gran_level = requested_gran.unwrap_or(0);
        setup_keys_table(conn, gran_level).with_context(|| {
            format!("Failed to initiliaze keys table with granularity level: {gran_level}")
        })?;
        return Ok(gran_level);
    }

    let (_, _, current_gran_level) = find_granlevel(conn)
        .with_context(|| "Failed to determine the current granularity level of the keys table")?;
    if let Some(new_gran_level) = requested_gran {
        // If we already have keys table and gran was provided, it means the user is changing
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
                conn.execute("DROP TABLE keys;", [])
                    .with_context(|| "Failed to drop the table keys to recreate")?;
                info!("Dropped 'keys' table, recreating with granularity {new_gran_level}.");
                setup_keys_table(conn, new_gran_level).with_context(|| {
                    format!("Failed to recreate keys with new granularity {new_gran_level}")
                })?;
            }
            "r" => {
                // Attempt to reorganize the existing table to fit the new granularity.p
                info!("Reorganizing 'keys' table from {current_gran_level} → {new_gran_level}.");
                reorganize_table(current_gran_level, new_gran_level).with_context(|| format!("Failed to reorganize keys table from: {current_gran_level} to: {new_gran_level}"))?;
            }
            _ => unreachable!(),
        }
        return Ok(new_gran_level);
    }

    info!("Table 'keys' already exists and no granularity change requested.");
    Ok(current_gran_level)
}

/// Creates the `keys` table and populates it with empty rows according to the granularity level.
fn setup_keys_table(conn: &Connection, g_level: u32) -> Result<()> {
    // SQLite does not have a dedicated date/time datatype. Instead, date and time values can stored as any of the following:
    let cq = "CREATE TABLE IF NOT EXISTS keys (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        left_clicks INTEGER NOT NULL,
        right_clicks INTEGER NOT NULL,
        middle_clicks INTEGER NOT NULL,
        key_presses INTEGER NOT NULL,
        cm_traveled INTEGER NOT NULL,
        dpi INTEGER NOT NULL,
        timestamp TEXT NOT NULL
    );";

    let (rows, interval_minutes) = match_gran_level(g_level);
    conn.execute(cq, [])
        .with_context(|| "Failed to create keys table")?;

    let today = Local::now().date_naive();
    let mut current_time = today.and_hms_opt(0, 0, 0).unwrap();
    let mut stmt = conn
        .prepare_cached(
            "INSERT INTO keys (left_clicks, right_clicks, middle_clicks, key_presses, cm_traveled, dpi, timestamp) 
         VALUES (0, 0, 0, 0, 0, 0, ?)",
        )
        .with_context(|| "Failed to prepare cached query to insert rows in keys table")?;
    debug!("today date_naive: {today}");
    for i in 0..rows {
        let timestamp = current_time.format("%H:%M").to_string();
        stmt.execute([timestamp.clone()]).with_context(|| {
            format!("Failed to insert row number: {i} with timestamp: {timestamp} into keys table")
        })?;
        current_time += Duration::minutes(interval_minutes as i64);
    }
    info!("Keys table created with granularity level {g_level}!");
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
fn find_granlevel(conn: &Connection) -> Result<(u32, u32, u32)> {
    let current_rowsn: u32 = conn
        .query_row("SELECT COUNT(*) FROM keys", [], |row| row.get(0))
        .with_context(|| "Failed to determine the number of rows in keys table")?;
    let (rows_n, interval, gran_level) = match current_rowsn {
        72 => (72, 15, 5),  // 15-minute intervals (72 rows)
        48 => (48, 30, 4),  // 30-minute intervals (48 rows)
        24 => (24, 60, 3),  // 1-hour intervals (24 rows)
        12 => (12, 120, 2), // 2-hour intervals (12 rows)
        6 => (6, 240, 1),   // 4-hour intervals (6 rows)
        _ => (1, 1440, 1),  // Default: No granularity (1 row)
    };
    Ok((rows_n, interval, gran_level))
}

fn reorganize_table(current_gran: u32, new_gran: u32) -> Result<()> {
    // determine the number of rows for each granularity level
    let (current_rows_n, _) = match_gran_level(current_gran);
    let (new_rows_n, _) = match_gran_level(new_gran);

    info!(
        "Reorganizing table from: {} rows/day to {} rows/day ",
        current_rows_n, new_rows_n
    );

    if current_gran < new_gran {
        let mut aux_conn =
            open_con().with_context(|| "Failed to open a connection with sqlite database")?;
        // Avoid corrupting the database
        let tx = aux_conn
            .transaction()
            .with_context(|| "Failed to start a new transaction")?;
        parse_to_higher_gran(&tx, new_gran, new_rows_n)
            .with_context(|| "Failed to parse keys table to higher granularity level")?;
        // we only commit if the parse don't fail
        tx.commit()
            .with_context(|| "Failed to commit the batch of transactions")?;
        Ok(())
    } else {
        let mut aux_conn = open_con()?;
        // Avoid corrupting the database
        let tx = aux_conn.transaction()?;
        let rows_to_merge = current_rows_n / new_rows_n;
        info!("Merging {rows_to_merge} rows");
        parse_to_lower_gran(&tx, new_gran, rows_to_merge)?;
        tx.commit()
            .with_context(|| "Failed to commit all transactions to organize keys table")?;
        Ok(())
    }
}

/// Fetches all rows from the `keys` table as fixed-size integer arrays with an associated timestamp.
/// `N`: The number of integer columns to extract (must be ≤ 6).
/// A vector of tuples where:
/// - The first element is an array `[i32; N]` containing the first `N` columns in order:
///   0: `left_clicks`, 1: `right_clicks`, 2: `middle_clicks`, 3: `key_presses`, 4: `cm_traveled`, 5: `dpi`.
/// - The second element is the timestamp String of the associated row.
/// - If `N` exceeds 6, the query will panic at runtime due to out-of-bounds access.
fn fetch_keys_columns<const N: usize>(conn: &Connection) -> Result<Vec<([i32; N], String)>> {
    let mut stmt = conn
        .prepare("SELECT left_clicks, right_clicks, middle_clicks, key_presses, cm_traveled, dpi, timestamp FROM keys")
        .with_context(|| "Failed to prepare SELECT statement")?;

    let rows = stmt
        .query_map([], |row| {
            let mut values = [0; N];
            for (i, item) in values.iter_mut().enumerate().take(N) {
                *item = row.get::<_, i32>(i)?;
            }
            let timestamp: String = row.get(6)?;
            Ok((values, timestamp))
        })
        .with_context(|| "Underlying sqlite query to retrieve rows failed")?;

    let mut result = Vec::new();
    for row_result in rows {
        result.push(row_result?);
    }
    Ok(result)
}

/// Reorganizes the `keys` table to a higher granularity level.
/// This function performs the following steps:
/// 1. Computes the total values of the five key columns (`left_clicks`, `right_clicks`, `middle_clicks`, `key_presses`, `cm_traveled`)
///    from the existing table to preserve cumulative data.
/// 2. Drops and recreates the `keys` table with the new granularity schema requested by the user.
/// 3. Precomputes average values for each key column based on the old totals and the desired number of new rows.
/// 4. Updates each row with the precomputed average values.
/// 5. Commits the transaction, ensuring that either all changes are applied or none if an error occurs.
///
/// - Accuracy may be imperfect when changing to a higher granularity because data is averaged and redistributed.
/// - All operations are performed in a single transaction to prevent partial updates or corruption.
fn parse_to_higher_gran(conn: &Connection, new_gran: u32, new_rows_n: u32) -> Result<()> {
    warn!("Cannot reorganize with full accuracy when changing to a higher granularity; summing and redistributing data...");
    let rows: Vec<([i32; 5], String)> = fetch_keys_columns::<5>(conn)
        .with_context(|| "Failed to fetch existent data from keys table")?;

    let mut old_totals = [0; 5];
    for (rows, _) in rows.iter() {
        for (total, val) in old_totals.iter_mut().zip(rows.iter()) {
            *total += val;
        }
    }

    // recreate table
    conn.execute("DROP TABLE IF EXISTS keys", [])
        .with_context(|| "Failed to drop existent keys table")?;
    setup_keys_table(conn, new_gran).with_context(|| {
        format!("Failed to recreate keys table with new granularity: {new_gran}")
    })?;

    // this will be sorted according to the SELECT query
    let averages: Vec<i32> = old_totals.iter().map(|&x| x / new_rows_n as i32).collect();
    for i in 1..=new_rows_n {
        conn.execute(
            "
        UPDATE keys
        SET left_clicks = ?, right_clicks = ?, middle_clicks = ?, key_presses = ?, cm_traveled = ?, dpi = ?
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
        )
        .with_context(|| format!("Failed to update data in row: {i} with average value"))?;
    }

    let rows: Vec<([i32; 5], String)> = fetch_keys_columns::<5>(conn)
        .with_context(|| "Failed to fetch data from new keys table")?;
    let mut new_totals = [0; 5];
    for (rows, _) in rows.iter() {
        for (total, val) in new_totals.iter_mut().zip(rows.iter()) {
            *total += val;
        }
    }

    info!("Table reorganization complete.");
    println!("Reorganization accuracy:");
    let labels = [
        "left_clicks",
        "right_clicks",
        "middle_clicks",
        "key_presses",
        "cm_traveled",
    ];
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

fn parse_to_lower_gran(conn: &Connection, new_gran: u32, rows_to_merge: u32) -> Result<()> {
    // go to parse_to_higher_gran to see why we open another connection here
    info!(
        "Reorganizing keys table to a lower granularity level. Each new row will be an aggregation of {} original rows.",
        rows_to_merge
    );

    // TODO: This amount of magic numbers is bothering me, i should create a struct for better readiblity
    let rows: Vec<([i32; 6], String)> =
        fetch_keys_columns(conn).with_context(|| "Failed to fetch data from keys table")?;

    let mut aggregated_rows = Vec::new();
    for chunk in rows.chunks(rows_to_merge as usize) {
        if chunk.is_empty() {
            continue;
        }

        let timestamp = chunk.first().unwrap().1.clone();
        let mut totals = [0; 6]; // [left_clicks, right_clicks, middle_clicks, key_presses, cm_traveled, dpi]

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

    conn.execute("DROP TABLE keys;", [])
        .with_context(|| "Failed to drop existent table")?;
    setup_keys_table(conn, new_gran).with_context(|| {
        format!("Failed to create new keys table with granularity level: {new_gran}")
    })?;

    let q = "INSERT INTO keys (left_clicks, right_clicks, middle_clicks, key_presses, cm_traveled, dpi, timestamp) 
         VALUES (0, 0, 0, 0, 0, 0, ?)";

    for row in aggregated_rows {
        conn.execute(q, params![row.0, row.1, row.2, row.3, row.4, row.5, row.6])
            .with_context(|| "Failed to add new aggregated rows in the keys table")?;
    }

    info!(
        "Successfully reorganized and updated data to new granularity level {}.",
        new_gran
    );

    Ok(())
}

pub fn clear_database() -> Result<()> {
    let db_path = program_data_dir()
        .with_context(|| "Could not determine directory to attempt to clear the sqlite database")?
        .join("data.db");

    match fs::remove_file(db_path.clone()) {
        Ok(_) => {
            info!(
                "Successfully removed database file: '{}'",
                db_path.display()
            );
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            // this isn't a failure case for us; it just means there's nothing to clean up.
            // We log it and return Ok.
            warn!(
                "Skipping cleanup: sqlite database not found at default location: '{}'",
                db_path.display()
            );
            Ok(())
        }
        Err(err) => Err(err).with_context(|| {
            format!(
                "Failed to delete sqlite database at: '{}'",
                db_path.display()
            )
        }),
    }
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
    Some(time_bucket_formated)
}

pub fn update_keyst(conn: &Connection, logger_data: &InputLogger) -> Result<()> {
    let now = Local::now();
    let now = format!("{:02}:{:02}", now.hour(), now.minute());

    let (_, interval, _) =
        find_granlevel(conn).with_context(|| "Failed to determine granularity level")?;
    let Some(bucket) = find_bucket(&now, interval) else {
        error!("No valid bucket found for time {}", now);
        // TODO: is this fine?
        return Ok(());
    };

    let q = "
            UPDATE keys
            SET left_clicks = ?,
                right_clicks = ?,
                middle_clicks = ?,
                key_presses = ?,
                cm_traveled = ?,
                dpi = ?
            WHERE timestamp = ?;
        ";

    let affected_rows = conn
        .execute(
            q,
            params![
                logger_data.left_clicks,
                logger_data.right_clicks,
                logger_data.middle_clicks,
                logger_data.key_presses,
                logger_data.cm_traveled,
                logger_data.mouse_dpi,
                bucket,
            ],
        )
        .with_context(|| "Could not update keys table, underlying sqlite call failed!")?;

    match affected_rows {
        1 => debug!(
            "Updated bucket '{}': LC=[{}], RC=[{}], MC=[{}], KP=[{}], MM=[{}], DPI=[{}]",
            bucket,
            logger_data.left_clicks,
            logger_data.right_clicks,
            logger_data.middle_clicks,
            logger_data.key_presses,
            logger_data.cm_traveled,
            logger_data.mouse_dpi
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
// TODO: why did i commented it?
// And i guess i should create a ticker that will reset keylogger values at each database interval.
pub fn get_keyst(conn: &Connection) -> Result<InputLogger> {
    let query = "
    SELECT left_clicks, right_clicks, middle_clicks, key_presses, cm_traveled,dpi
    FROM keys
    LIMIT 1;
    ";

    let mut stmt = conn.prepare(query)?;
    // Old: Assuming there is only one row
    // new: I mean how does it not fail if there is more than one row in the fucking table? is the old
    // me that stupid or is the new me dumb?
    let row = stmt.query_row([], |row| {
        let k = InputLogger {
            left_clicks: row.get(0)?,
            right_clicks: row.get(1)?,
            middle_clicks: row.get(2)?,
            key_presses: row.get(3)?,
            cm_traveled: row.get(4)?,
            mouse_dpi: row.get(5)?,
            ..Default::default()
        };
        Ok(k)
    })?;

    Ok(row)
}

// Need to be owned by the caller, Vec seems better in that case.
pub fn get_proct(conn: &Connection) -> Result<Vec<ProcessInfo>> {
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

pub fn open_con() -> Result<Connection> {
    let db_path = program_data_dir()
        .with_context(|| "Could not determine directory to attempt to clear the sqlite database")?
        .join("data.db");
    info!("Connection open with database: [{}]", db_path.display());

    let conn = Connection::open_with_flags(
        db_path.clone(),
        OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .with_context(|| {
        format!(
            "Failed to open database connection with sqlite database at: {}",
            db_path.display()
        )
    })?;
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
