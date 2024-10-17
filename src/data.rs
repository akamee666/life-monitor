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
        // This is cheap because both are inside an Arc.
        let k: KeyLogger = keylogger.clone();
        let con = self.con.clone();

        // The only way this task can fail is if the underlying SQLite call fails, which will
        // return a Result<JoinHandle<rusqlite::Error>>.
        // Unwrap the Result<JoinHandle> should bet safe in that case, i think.
        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            update_keyst(&con, &k).map_err(DataStoreError::DbError)
        })
        .await
        .unwrap()
    }

    async fn store_proc_data(&self, _proc_info: &[ProcessInfo]) -> Result<(), DataStoreError> {
        let _con = self.con.clone();
        //tokio::task::spawn_blocking(move || {
        //    let con = con.lock().unwrap();
        //
        //    update_proct(&con, proc_info).map_err(DataStoreError::DbError)
        //})
        //.await
        //.unwrap()
        Ok(())
    }

    async fn get_keys_data(&self) -> Result<KeyLogger, DataStoreError> {
        let con = self.con.clone();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            get_keyst(&con).map_err(DataStoreError::DbError)
        })
        .await
        .unwrap()
    }

    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>, DataStoreError> {
        let con = self.con.clone();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            get_proct(&con).map_err(DataStoreError::DbError)
        })
        .await
        .unwrap()
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
        to_api(&self.client, &self.config, keylogger, reqwest::Method::POST).await?;

        // This would be fine if i handle the errors from api call correctly at to_api function.
        Ok(())
    }

    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<(), DataStoreError> {
        to_api(
            &self.client,
            &self.config,
            &proc_info.to_vec(),
            reqwest::Method::POST,
        )
        .await?;

        Ok(())
    }

    async fn get_keys_data(&self) -> Result<KeyLogger, DataStoreError> {
        // That is horrible lmao.
        // FIX:
        let k = KeyLogger {
            ..Default::default()
        };
        let ret = to_api(&self.client, &self.config, &k, reqwest::Method::GET)
            .await?
            .unwrap();
        Ok(ret)
    }

    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>, DataStoreError> {
        // That is horrible lmao.
        // FIX:
        let p: Vec<ProcessInfo> = Vec::new();
        let ret = to_api(&self.client, &self.config, &p, reqwest::Method::GET)
            .await?
            .unwrap();
        Ok(ret)
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
}

impl fmt::Display for DataStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Add your actual database query here
        match self {
            DataStoreError::ApiError(e) => write!(f, "API Error: {}", e),
            DataStoreError::DbError(e) => write!(f, "Database Error: {}", e),
        }
    }
}

impl std::error::Error for DataStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DataStoreError::ApiError(e) => Some(e),
            DataStoreError::DbError(e) => Some(e),
        }
    }
}

// Implement From to automatically convert reqwest::Error and rusqlite::Error
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
