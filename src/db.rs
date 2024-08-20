use log::{debug, info};
use sqlite::Connection;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn get_db_conn() -> Result<Connection, Box<dyn std::error::Error>> {
    info!("Sarting db connection");
    // Get the LOCALAPPDATA environment variable or use default path
    let local_app_data = env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\Temp".into());
    let mut path = PathBuf::from(&local_app_data);
    path.push("akame_monitor");
    path.push("tracked_data.db");

    // Log the path for debugging
    info!("Path for db: {:?}", path);

    // Ensure the parent directory exists
    if let Some(parent_dir) = path.parent() {
        fs::create_dir_all(parent_dir)?;
    }

    // Create the database file and open a connection
    let conn = if !Path::new(&path).exists() {
        // File does not exist, create a new database file
        let conn = Connection::open(&path)?;
        info!("Database created at: {}", path.display());

        // Initialize your database schema here (if necessary)
        // e.g., conn.execute("CREATE TABLE ...", [])?;

        conn
    } else {
        // File exists, open the existing database
        info!("Database already exists at: {}", path.display());
        Connection::open(&path)?
    };

    Ok(conn)
}

pub fn upload_data_to_db() -> Result<(), Box<dyn std::error::Error>> {
    debug!("Sending Data to database");
    let conn = get_db_conn()?;
    // Use conn to perform database operations
    Ok(())
}
