// Requesting processes and keylogger data from my api>>
// /v1/keys - POST || GET
// /v1/proc - POST || GET
// Both wait for a json.
//
use crate::{keylogger::KeyLogger, processinfo::ProcessInfo};
use reqwest::Client;
use serde_json::json;
use std::env;
use std::error::Error;
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

    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config_str = std::fs::read_to_string(path)?;
        let config: ApiConfig = serde_json::from_str(&config_str)?;
        info!("config: {:?}", config);
        Ok(config)
    }
}

pub trait ApiSendable {
    fn get_route(&self, config: &ApiConfig) -> String;
    fn to_json(&self) -> serde_json::Value;
}

impl ApiSendable for KeyLogger {
    fn get_route(&self, config: &ApiConfig) -> String {
        config.keys_endpoint.clone()
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "left_clicks": self.left_clicks,
            "right_clicks": self.right_clicks,
            "middle_clicks": self.middle_clicks,
            "keys_pressed": self.keys_pressed,
            "mouse_moved_cm": self.mouse_moved_cm,
            "mouse_dpi": self.mouse_settings.dpi,
        })
    }
}

impl ApiSendable for Vec<ProcessInfo> {
    fn get_route(&self, config: &ApiConfig) -> String {
        config.proc_endpoint.clone()
    }

    fn to_json(&self) -> serde_json::Value {
        // FIX:
        json!(self
            .iter()
            .map(|info| {
                json!({
                    "name": info.name,
                    "time_spent": info.time_spent,
                    "instance": info.instance,
                    "window_class": info.window_class,
                })
            })
            .collect::<Vec<_>>())
    }
}

pub async fn send_to_api<T: ApiSendable>(
    client: &Client,
    config: &ApiConfig,
    data: &T,
) -> Result<(), Box<dyn Error>> {
    let url = format!("{}{}", config.base_url, data.get_route(config));
    debug!("Sending data to URL: {}", url);
    let json_data = data.to_json();

    let mut request = client.post(&url).json(&json_data);
    if let Some(api_key) = &config.api_key {
        request = request.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = request.send().await?;

    if response.status().is_success() {
        debug!("Data sent successfully to {}", url);
        Ok(())
    } else {
        Err(format!("API request failed with status: {}", response.status()).into())
    }
}

// FIX:
#[cfg(test)]
mod tests {
    use super::*;
    use mockito::mock;
    use mockito::server_url;
    use reqwest::Client;

    fn mock_api_config() -> ApiConfig {
        let base_url = server_url();
        ApiConfig {
            base_url,
            api_key: None, // For testing, you may leave it None
            keys_endpoint: "/v1/keys".to_string(),
            proc_endpoint: "/v1/proc".to_string(),
        }
    }

    #[tokio::test]
    async fn test_send_keylogger_data() {
        let m = mock("POST", "/v1/keys")
            .match_body(mockito::Matcher::Json(json!({
                "left_clicks": 100,
                "right_clicks": 50,
                "middle_clicks": 10,
                "keys_pressed": 1000,
                "pixels_moved": 5000.5,
                "mouse_moved_cm": 200.0,
            })))
            .with_status(200)
            .create();

        let client = Client::new();
        let config = mock_api_config();

        let key_logger = KeyLogger {
            left_clicks: 100,
            right_clicks: 50,
            middle_clicks: 10,
            keys_pressed: 1000,
            pixels_moved: 5000.5,
            mouse_moved_cm: 200.0,
            ..Default::default()
        };

        let result = send_to_api(&client, &config, &key_logger).await;
        assert!(result.is_ok());
        m.assert();
    }

    #[tokio::test]
    async fn test_send_process_info() {
        let m = mock("POST", "/v1/proc")
            .match_body(mockito::Matcher::Json(json!([
                {
                    "name": "Process1",
                    "time_spent": 3600,
                    "instance": "Instance1",
                    "window_class": "Class1",
                },
                {
                    "name": "Process2",
                    "time_spent": 1800,
                    "instance": "Instance2",
                    "window_class": "Class2",
                }
            ])))
            .with_status(200)
            .create();

        let client = Client::new();
        let config = mock_api_config();

        let process_info = vec![
            ProcessInfo {
                name: "Process1".to_string(),
                time_spent: 3600,
                instance: "Instance1".to_string(),
                window_class: "Class1".to_string(),
            },
            ProcessInfo {
                name: "Process2".to_string(),
                time_spent: 1800,
                instance: "Instance2".to_string(),
                window_class: "Class2".to_string(),
            },
        ];

        let result = send_to_api(&client, &config, &process_info).await;
        assert!(result.is_ok());
        m.assert();
    }

    #[tokio::test]
    async fn test_api_error_response() {
        let m = mock("POST", "/v1/keys").with_status(500).create();

        let client = Client::new();
        let config = mock_api_config();

        let key_logger = KeyLogger {
            left_clicks: 100,
            right_clicks: 50,
            middle_clicks: 10,
            keys_pressed: 1000,
            pixels_moved: 5000.5,
            mouse_moved_cm: 200.0,
            ..Default::default()
        };

        let result = send_to_api(&client, &config, &key_logger).await;
        assert!(result.is_err());
        m.assert();
    }

    #[test]
    fn test_keylogger_to_json() {
        let key_logger = KeyLogger {
            left_clicks: 100,
            right_clicks: 50,
            middle_clicks: 10,
            keys_pressed: 1000,
            pixels_moved: 5000.5,
            mouse_moved_cm: 200.0,
            ..Default::default()
        };

        let json = key_logger.to_json();
        assert_eq!(
            json,
            json!({
                "left_clicks": 100,
                "right_clicks": 50,
                "middle_clicks": 10,
                "keys_pressed": 1000,
                "pixels_moved": 5000.5,
                "mouse_moved_cm": 200.0,
            })
        );
    }

    #[test]
    fn test_process_info_to_json() {
        let process_info = vec![ProcessInfo {
            name: "Process1".to_string(),
            time_spent: 3600,
            instance: "Instance1".to_string(),
            window_class: "Class1".to_string(),
        }];

        let json = process_info.to_json();
        assert_eq!(
            json,
            json!([{
                "name": "Process1",
                "time_spent": 3600,
                "instance": "Instance1",
                "window_class": "Class1",
            }])
        );
    }
}
