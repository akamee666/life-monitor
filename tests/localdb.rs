use mockall::mock;
use mockall::predicate::*;

use crate::db_operations::{
    get_input_data, get_process_data, send_to_input_table, send_to_process_table,
};
use crate::{KeyLogger, ProcessInfo};
use rusqlite::{Connection, Result as SqliteResult};
use std::error::Error;

// Helper function to create an in-memory database for testing
fn create_test_db() -> SqliteResult<Connection> {
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
        mouse_moved_cm: 50,
        pixels_moved: 0.0,
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
    assert_eq!(result.mouse_moved_cm, 25);

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
