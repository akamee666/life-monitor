use crate::common::*;
use crate::DataStore;

use anyhow::*;

use reqwest::Client;
use serde_json::json;

use tracing::*;

use std::env;
use std::result::Result::Ok;

#[derive(Clone, serde::Deserialize, Debug)]
pub struct ApiConfig {
    base_url: String,
    api_key: Option<String>,
    keys_endpoint: String,
    proc_endpoint: String,
}

#[derive(Debug, Clone)]
pub struct RemoteDb {
    client: Client,
    config: ApiConfig,
}

impl RemoteDb {
    pub fn new(config_path: &String) -> Result<Self> {
        info!("Config file name: '{}'", config_path);

        let config = ApiConfig::from_file(config_path)?;
        let client = Client::builder().build()?;
        info!("Backend using API sucessfully initialized.");
        Ok(Self { client, config })
    }
}

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
// TODO: CHANGE REMOTE CODE FROM backend to HERE

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.example.com".to_string(),
            api_key: None,
            keys_endpoint: "/v1/keys".to_string(),
            proc_endpoint: "/v1/proc".to_string(),
        }
    }
}

impl ApiConfig {
    #[allow(dead_code)]
    pub fn from_env() -> Self {
        let base_url =
            env::var("API_BASE_URL").unwrap_or_else(|_| "https://api.example.com".to_string());
        let api_key = env::var("API_KEY").ok();
        let keys_endpoint =
            env::var("API_KEYS_ENDPOINT").unwrap_or_else(|_| "/v1/keys".to_string());
        let proc_endpoint =
            env::var("API_PROC_ENDPOINT").unwrap_or_else(|_| "/v1/proc".to_string());

        Self {
            base_url,
            api_key,
            keys_endpoint,
            proc_endpoint,
        }
    }

    /// This will panic if the file operation fails. It should return BackEndError maybe? or BackEndError::APIError?
    pub fn from_file(path: &str) -> Result<Self> {
        let config_str = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read contents of config file: {path}"))?;
        let mut config: ApiConfig = serde_json::from_str(&config_str).with_context(|| {
            format!("Failed to parse the contents of file: {path} to a valid json")
        })?;
        // debug!("API Config: {:#?}", config);

        if config.api_key.is_none() {
            info!("API key found in the config file");
            match env::var("API_KEY") {
                Ok(key) => {
                    info!("API key found in $API_KEY");
                    config.api_key = Some(key);
                }

                Err(err) => {
                    warn!("Failed to get $API_KEY from environment: {err:?}");
                    warn!("Calls to remote api will be made without a key!");
                }
            }
        }
        Ok(config)
    }
}

pub trait ApiSendable {
    fn get_route(&self, config: &ApiConfig) -> String;
    fn to_json(&self) -> serde_json::Value;
    fn from_json(json: serde_json::Value) -> Self;
}

impl ApiSendable for InputLogger {
    fn get_route(&self, config: &ApiConfig) -> String {
        config.keys_endpoint.clone()
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "left_clicks": self.left_clicks,
            "right_clicks": self.right_clicks,
            "middle_clicks": self.middle_clicks,
            "key_presses": self.key_presses,
            "pixels_traveled": self.pixels_traveled,
        })
    }
    fn from_json(json: serde_json::Value) -> Self {
        serde_json::from_value(json).unwrap_or_else(|err| {
            error!("Failed to parse data from json!\n Error: {err}");
            panic!();
        })
    }
}

impl ApiSendable for Vec<ProcessInfo> {
    fn get_route(&self, config: &ApiConfig) -> String {
        config.proc_endpoint.clone()
    }

    fn to_json(&self) -> serde_json::Value {
        json!(self
            .iter()
            .map(|info| {
                json!({
                    "w_name": info.w_name,
                    "w_time": info.w_time,
                    "w_class": info.w_class,
                })
            })
            .collect::<Vec<_>>())
    }
    fn from_json(json: serde_json::Value) -> Vec<ProcessInfo> {
        serde_json::from_value(json).expect("Failed to deserialize ProcessInfo array")
    }
}

pub async fn to_api<T: ApiSendable + Sized + std::fmt::Debug>(
    client: &Client,
    config: &ApiConfig,
    data: &T,
    method: reqwest::Method,
) -> Result<Option<T>> {
    let url = format!("{}{}", config.base_url, data.get_route(config));
    debug!("Request {} to: {}", method, url);
    let mut req = match method {
        reqwest::Method::POST => {
            let mut req = client.post(&url);
            let data_j = data.to_json();
            debug!("Data Sent: {:#?}", data);
            req = req.json(&data_j);
            req
        }
        reqwest::Method::GET => client.get(&url),
        _ => {
            error!("API only accepts GET or POST method.");
            panic!();
        }
    };
    if let Some(api_key) = &config.api_key {
        req = req.header("Authorization", format!("Bearer {}", api_key.trim()));
    }
    req = req
        .header("User-Agent", "AkameSpy/1.0")
        .header("Accept", "application/json");

    // Debug log the full request
    let request = req.build()?;
    debug!("Request headers: {:#?}", request.headers());

    let response = client.execute(request).await?;
    let status = response.status();
    // Change it to Match method.
    if status.is_success() {
        if method == reqwest::Method::GET {
            let data_j: serde_json::Value = response.json().await?;
            let data: T = T::from_json(data_j);
            debug!("Data received: {:#?}", data);
            return Ok(Some(data));
        } else {
            let text = response.text().await?;
            debug!("Data received: {:#?}", text);
            return Ok(None);
        }
    } else {
        error!("Request failed! Status code: {}", response.status());
        // Log the response body for more details
        let body = response.text().await?;
        error!("Response body: {}", body);
    }
    debug!("{} returned status: {}", url, status);
    Ok(None)
}
