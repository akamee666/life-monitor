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
    pub fn new(config_path: &str) -> Result<Self> {
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
            .await?
            .ok_or_else(|| anyhow!("Failed to get keys data from API"))?;
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
            .await?
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
            base_url: "http://localhost:3000".to_string(),
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
            env::var("API_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
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

    /// This will panic if the file operation fails.
    pub fn from_file(path: &str) -> Result<Self> {
        let config_str = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read contents of config file: {path}"))?;
        let mut config: ApiConfig = serde_json::from_str(&config_str).with_context(|| {
            format!("Failed to parse the contents of file: {path} to a valid json")
        })?;

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

    let request = req.build()?;
    debug!("Request headers: {:#?}", request.headers());

    let response = client.execute(request).await?;
    let status = response.status();
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
    }
    debug!("{} returned status: {}", url, status);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn unique_temp_path(name: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("life-monitor-{name}-{suffix}.json"))
    }

    /// Verifies that `ApiConfig::from_env` falls back to the documented defaults when
    /// the process environment does not provide any overrides.
    #[test]
    fn api_config_from_env_uses_defaults_when_env_is_missing() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("API_BASE_URL");
        std::env::remove_var("API_KEY");
        std::env::remove_var("API_KEYS_ENDPOINT");
        std::env::remove_var("API_PROC_ENDPOINT");

        let config = ApiConfig::from_env();

        assert_eq!(config.base_url, "http://localhost:3000");
        assert_eq!(config.api_key, None);
        assert_eq!(config.keys_endpoint, "/v1/keys");
        assert_eq!(config.proc_endpoint, "/v1/proc");
    }

    /// Verifies that `ApiConfig::from_env` honors explicit environment overrides so tests
    /// and deployments can redirect requests without editing source-controlled config files.
    #[test]
    fn api_config_from_env_uses_environment_overrides() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("API_BASE_URL", "https://example.com");
        std::env::set_var("API_KEY", "top-secret");
        std::env::set_var("API_KEYS_ENDPOINT", "/custom/keys");
        std::env::set_var("API_PROC_ENDPOINT", "/custom/procs");

        let config = ApiConfig::from_env();

        assert_eq!(config.base_url, "https://example.com");
        assert_eq!(config.api_key.as_deref(), Some("top-secret"));
        assert_eq!(config.keys_endpoint, "/custom/keys");
        assert_eq!(config.proc_endpoint, "/custom/procs");
    }

    /// Verifies that `ApiConfig::from_file` accepts JSON config files and uses `$API_KEY`
    /// as a fallback when the file intentionally omits the secret.
    #[test]
    fn api_config_from_file_uses_environment_key_fallback() -> Result<()> {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("API_BASE_URL");
        std::env::set_var("API_KEY", "fallback-key");

        let config_path = unique_temp_path("api-config");
        fs::write(
            &config_path,
            r#"{
                "base_url": "https://example.com",
                "api_key": null,
                "keys_endpoint": "/v2/keys",
                "proc_endpoint": "/v2/proc"
            }"#,
        )?;

        let config = ApiConfig::from_file(config_path.to_str().unwrap())?;

        assert_eq!(config.base_url, "https://example.com");
        assert_eq!(config.api_key.as_deref(), Some("fallback-key"));
        assert_eq!(config.keys_endpoint, "/v2/keys");
        assert_eq!(config.proc_endpoint, "/v2/proc");

        let _ = fs::remove_file(config_path);
        Ok(())
    }

    /// Verifies that the input payload sent to the remote API contains only the fields
    /// that the current backend serializes for key metrics.
    #[test]
    fn input_logger_to_json_serializes_expected_fields() {
        let logger = InputLogger {
            left_clicks: 1,
            right_clicks: 2,
            middle_clicks: 3,
            key_presses: 4,
            pixels_traveled: 5,
            cm_traveled: 6.5,
            ..Default::default()
        };

        let json = logger.to_json();

        assert_eq!(json["left_clicks"], 1);
        assert_eq!(json["right_clicks"], 2);
        assert_eq!(json["middle_clicks"], 3);
        assert_eq!(json["key_presses"], 4);
        assert_eq!(json["pixels_traveled"], 5);
        assert!(json.get("cm_traveled").is_none());
    }

    /// Verifies that process vectors round-trip through the remote JSON contract without
    /// losing per-window names, classes, or focused time values.
    #[test]
    fn process_info_vectors_round_trip_through_json() {
        let processes = vec![
            ProcessInfo {
                w_name: "Browser".to_string(),
                w_time: 12,
                w_class: "firefox".to_string(),
            },
            ProcessInfo {
                w_name: "Editor".to_string(),
                w_time: 30,
                w_class: "nvim".to_string(),
            },
        ];

        let json = processes.to_json();
        let parsed = Vec::<ProcessInfo>::from_json(json);

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].w_name, "Browser");
        assert_eq!(parsed[0].w_class, "firefox");
        assert_eq!(parsed[1].w_name, "Editor");
        assert_eq!(parsed[1].w_time, 30);
    }
}
