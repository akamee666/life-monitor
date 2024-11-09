//! This file is responsible to make it easier change between using an API or a SQLite as database
//! storage.

use crate::keylogger::KeyLogger;
use crate::ProcessInfo;
use tracing::*;

use reqwest::Client;
use rusqlite::Connection;

use crate::storage::api::*;
use crate::storage::localdb::*;

use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

#[allow(async_fn_in_trait)]
pub trait DataStore {
    async fn store_keys_data(&self, keylogger: &KeyLogger) -> Result<(), BackEndError>;
    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<(), BackEndError>;
    async fn get_keys_data(&self) -> Result<KeyLogger, BackEndError>;
    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>, BackEndError>;
}

#[derive(Debug, Clone)]
pub struct LocalDbStore {
    con: Arc<Mutex<Connection>>,
}

impl LocalDbStore {
    pub fn new(gran_level: Option<u32>, should_clear: bool) -> Result<Self, BackEndError> {
        if should_clear {
            info!("Clean argument provided, cleaning database!");

            match clean_database() {
                Ok(_) => {}
                Err(e) => {
                    warn!("Could not delete database, because of error: {e}. Most likely the database does not exist already, no need to crash.");
                }
            }
        };

        let conn = open_con()?;
        initialize_database(&conn, gran_level)?;

        info!("Backend using SQLite sucessfully initialized.");

        Ok(Self {
            con: Arc::new(Mutex::new(conn)),
        })
    }
}

impl DataStore for LocalDbStore {
    async fn store_keys_data(&self, keylogger: &KeyLogger) -> Result<(), BackEndError> {
        let k: KeyLogger = keylogger.clone();
        let con = self.con.clone();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            update_keyst(&con, &k).map_err(BackEndError::DbError)
        })
        .await?
    }

    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<(), BackEndError> {
        let con = self.con.clone();
        let procs = proc_info.to_vec();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            update_proct(&con, &procs).map_err(BackEndError::DbError)
        })
        .await?
    }

    async fn get_keys_data(&self) -> Result<KeyLogger, BackEndError> {
        let con = self.con.clone();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            get_keyst(&con).map_err(BackEndError::DbError)
        })
        .await?
    }

    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>, BackEndError> {
        let con = self.con.clone();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            get_proct(&con).map_err(BackEndError::DbError)
        })
        .await?
    }
}

#[derive(Debug, Clone)]
pub struct ApiStore {
    client: Client,
    config: ApiConfig,
}

impl ApiStore {
    pub fn new(config_path: &String) -> Result<Self, BackEndError> {
        info!("Config file name: {:?}", config_path);

        let config = ApiConfig::from_file(config_path).unwrap_or_else(|err| {
            error!("Could not parse {config_path}. Error: {err}");
            panic!()
        });

        // Using builder to handle possible errors.
        let client = Client::builder().build()?;
        info!("Backend using API sucessfully initialized.");

        Ok(Self { client, config })
    }
}

impl DataStore for ApiStore {
    async fn store_keys_data(&self, keylogger: &KeyLogger) -> Result<(), BackEndError> {
        to_api(&self.client, &self.config, keylogger, reqwest::Method::POST).await?;

        Ok(())
    }

    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<(), BackEndError> {
        to_api(
            &self.client,
            &self.config,
            &proc_info.to_vec(),
            reqwest::Method::POST,
        )
        .await?;

        Ok(())
    }

    async fn get_keys_data(&self) -> Result<KeyLogger, BackEndError> {
        let k = KeyLogger {
            ..Default::default()
        };
        let result = to_api(&self.client, &self.config, &k, reqwest::Method::GET)
            .await?
            .unwrap();
        Ok(result)
    }

    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>, BackEndError> {
        let p: Vec<ProcessInfo> = Vec::new();
        let result = to_api(&self.client, &self.config, &p, reqwest::Method::GET)
            .await?
            .unwrap();
        Ok(result)
    }
}

#[derive(Debug, Clone)]
pub enum StorageBackend {
    Local(LocalDbStore),
    Api(ApiStore),
}

impl DataStore for StorageBackend {
    async fn store_keys_data(&self, keylogger: &KeyLogger) -> Result<(), BackEndError> {
        match self {
            StorageBackend::Local(db) => db.store_keys_data(keylogger).await,
            StorageBackend::Api(api) => api.store_keys_data(keylogger).await,
        }
    }

    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<(), BackEndError> {
        match self {
            StorageBackend::Local(db) => db.store_proc_data(proc_info).await,
            StorageBackend::Api(api) => api.store_proc_data(proc_info).await,
        }
    }

    async fn get_keys_data(&self) -> Result<KeyLogger, BackEndError> {
        match self {
            StorageBackend::Local(db) => db.get_keys_data().await,
            StorageBackend::Api(api) => api.get_keys_data().await,
        }
    }

    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>, BackEndError> {
        match self {
            StorageBackend::Local(db) => db.get_proc_data().await,
            StorageBackend::Api(api) => api.get_proc_data().await,
        }
    }
}

#[derive(Debug)]
pub enum BackEndError {
    ApiError(reqwest::Error),
    DbError(rusqlite::Error),
    TaskError(tokio::task::JoinError),
}

impl fmt::Display for BackEndError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackEndError::ApiError(e) => {
                error!("API Error: {}", e); // Automatically log the error
                write!(f, "API Error: {}", e)
            }
            BackEndError::DbError(e) => {
                error!("Database Error: {}", e); // Automatically log the error
                write!(f, "Database Error: {}", e)
            }
            BackEndError::TaskError(e) => {
                error!("Task Error: {}", e); // Automatically log the error
                write!(f, "Task Error: {}", e)
            }
        }
    }
}

impl std::error::Error for BackEndError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BackEndError::ApiError(e) => Some(e),
            BackEndError::DbError(e) => Some(e),
            BackEndError::TaskError(e) => Some(e),
        }
    }
}

impl From<reqwest::Error> for BackEndError {
    fn from(err: reqwest::Error) -> Self {
        BackEndError::ApiError(err)
    }
}

impl From<rusqlite::Error> for BackEndError {
    fn from(err: rusqlite::Error) -> Self {
        BackEndError::DbError(err)
    }
}

impl From<tokio::task::JoinError> for BackEndError {
    fn from(err: tokio::task::JoinError) -> Self {
        BackEndError::TaskError(err)
    }
}
