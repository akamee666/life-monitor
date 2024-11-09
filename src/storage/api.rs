use crate::keylogger::KeyLogger;
use crate::ProcessInfo;

use reqwest::Client;
use serde_json::json;

use core::panic;
use std::env;

use tracing::*;

#[derive(Clone, serde::Deserialize, Debug)]
pub struct ApiConfig {
    base_url: String,
    api_key: Option<String>,
    keys_endpoint: String,
    proc_endpoint: String,
}

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

    pub fn from_file(path: &str) -> Result<Self, std::io::Error> {
        let config_str = std::fs::read_to_string(path)?;
        let mut config: ApiConfig = serde_json::from_str(&config_str)?;

        info!("API Config: {:#?}", config);
        if config.api_key.is_none() {
            info!("API key not provided by config file. Attempting to find using env variable.");

            if let Ok(api_key) = env::var("API_KEY") {
                info!("API key find on env variable.");
                config.api_key = Some(api_key);
            } else {
                warn!("API key not found neither on env variable or config file.");
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

impl ApiSendable for KeyLogger {
    fn get_route(&self, config: &ApiConfig) -> String {
        config.keys_endpoint.clone()
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "t_lc": self.t_lc,
            "t_rc": self.t_rc,
            "t_mc": self.t_mc,
            "t_kp": self.t_kp,
            "t_mm": self.t_mm,
            "dpi": self.mouse_settings.dpi,
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
                    "w_instance": info.w_instance,
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
) -> Result<Option<T>, reqwest::Error> {
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
