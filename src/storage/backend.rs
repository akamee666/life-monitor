//! This file is responsible to make it easier change between using an API or a SQLite as database
//! storage.
use crate::common::*;
use crate::storage::localdb::*;

use rusqlite::Connection;

use std::sync::{Arc, Mutex};

use anyhow::*;

use tracing::*;

#[cfg(feature = "x11")]
use reqwest::*;

#[allow(async_fn_in_trait)]
pub trait DataStore {
    async fn store_keys_data(&self, keylogger: &InputLogger) -> Result<()>;
    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<()>;
    async fn get_keys_data(&self) -> Result<InputLogger>;
    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>>;
}

#[derive(Debug, Clone)]
pub struct LocalDb {
    con: Arc<Mutex<Connection>>,
}

impl LocalDb {
    pub fn new(req_gran_level: Option<u32>, should_clear: bool) -> Result<Self> {
        if should_clear {
            info!("Clean argument provided, cleaning database!");
            clear_database().context("Failed to clear database")?;
        };

        let conn = open_con().context("Failed to open connection with sqlite database")?;
        setup_database(&conn, req_gran_level)
            .context("Failed to properly setup sqlite database")?;

        info!("Backend using SQLite sucessfully initialized.");
        Ok(Self {
            con: Arc::new(Mutex::new(conn)),
        })
    }
}

impl DataStore for LocalDb {
    async fn store_keys_data(&self, keylogger: &InputLogger) -> Result<()> {
        let k: InputLogger = keylogger.clone();
        let con = self.con.clone();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            update_keyst(&con, &k).context("Failed to update keystroke data in the sqlite database")
        })
        .await?
    }

    async fn get_keys_data(&self) -> Result<InputLogger> {
        let con = self.con.clone();

        let input_logger_res = tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            get_keyst(&con).with_context(|| "Failed to get keys data from the database")
        })
        .await??; // <- first ? on JoinError, second ? on anyhow::Error

        Ok(input_logger_res)
    }

    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<()> {
        let con = self.con.clone();
        let procs = proc_info.to_vec();

        tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            update_proct(&con, &procs).context("Failed to update process data in the database")
        })
        .await?
    }

    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>> {
        let con = self.con.clone();

        let res = tokio::task::spawn_blocking(move || {
            let con = con.lock().unwrap();
            get_proct(&con).context("Failed to get process data from the database")
        })
        .await??;
        Ok(res)
    }
}

#[derive(Debug, Clone)]
#[cfg(feature = "remote")]
pub struct RemoteDb {
    client: Client,
    config: ApiConfig,
}

#[cfg(feature = "remote")]
impl RemoteDb {
    pub fn new(config_path: &String) -> Result<Self> {
        info!("Config file name: '{}'", config_path);

        let config = ApiConfig::from_file(config_path)?;
        let client = Client::builder().build()?;
        info!("Backend using API sucessfully initialized.");
        Ok(Self { client, config })
    }
}

#[cfg(feature = "remote")]
impl DataStore for RemoteDb {
    async fn get_keys_data(&self) -> Result<InputLogger> {
        let k = InputLogger {
            ..Default::default()
        };
        let result = to_api(&self.client, &self.config, &k, reqwest::Method::GET)
            .await
            .context("API request for key data failed")?
            .ok_or_else(|| {
                anyhow!("API returned no key data, but expected a InputLogger object")
            })?;
        Ok(result)
    }

    async fn store_keys_data(&self, keylogger: &InputLogger) -> Result<()> {
        to_api(&self.client, &self.config, keylogger, reqwest::Method::POST)
            .await
            .context("Failed to send key data to the API")?;
        Ok(())
    }

    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>> {
        let p: Vec<ProcessInfo> = Vec::new();
        let result = to_api(&self.client, &self.config, &p, reqwest::Method::GET)
            .await
            .context("API request for process data failed")?
            .ok_or_else(|| {
                anyhow!("API returned no process data, but expected a vector of processes")
            })?;
        Ok(result)
    }

    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<()> {
        to_api(
            &self.client,
            &self.config,
            &proc_info.to_vec(),
            reqwest::Method::POST,
        )
        .await
        .context("Failed to send process data to the API")?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum StorageBackend {
    Local(LocalDb),
    #[cfg(feature = "remote")]
    Api(RemoteDb),
}

impl DataStore for StorageBackend {
    async fn store_keys_data(&self, keylogger: &InputLogger) -> Result<()> {
        match self {
            StorageBackend::Local(db) => db.store_keys_data(keylogger).await,
            #[cfg(feature = "remote")]
            StorageBackend::Api(api) => api.store_keys_data(keylogger).await,
        }
    }

    async fn store_proc_data(&self, proc_info: &[ProcessInfo]) -> Result<()> {
        match self {
            StorageBackend::Local(db) => db.store_proc_data(proc_info).await,
            #[cfg(feature = "remote")]
            StorageBackend::Api(api) => api.store_proc_data(proc_info).await,
        }
    }

    async fn get_keys_data(&self) -> Result<InputLogger> {
        match self {
            StorageBackend::Local(db) => db.get_keys_data().await,
            #[cfg(feature = "remote")]
            StorageBackend::Api(api) => api.get_keys_data().await,
        }
    }

    async fn get_proc_data(&self) -> Result<Vec<ProcessInfo>> {
        match self {
            StorageBackend::Local(db) => db.get_proc_data().await,
            // .with_context(|| "Failed to retrieve process data from local storage"),
            #[cfg(feature = "remote")]
            StorageBackend::Api(api) => api.get_proc_data().await,
            // .with_context(|| "Failed to retrieve process data from API"),
        }
    }
}
