use crate::{keylogger::KeyLogger, processinfo::ProcessInfo};
use reqwest::Client;
use serde_json::json;
use std::error::Error;
use tracing::*;

// Requesting processes and keylogger data from my api>>
// /v1/keys - POST || GET
// /v1/proc - POST || GET
// Both wait for a json.
// fantasyrealm.xyz
//
pub trait ApiSendable {
    fn get_route(&self) -> &str;
    fn to_json(&self) -> serde_json::Value;
}

impl ApiSendable for KeyLogger {
    fn get_route(&self) -> &str {
        "/v1/keys"
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "left_clicks": self.left_clicks,
            "right_clicks": self.right_clicks,
            "middle_clicks": self.middle_clicks,
            "keys_pressed": self.keys_pressed,
            "pixels_moved": self.pixels_moved,
            "mouse_moved_cm": self.mouse_moved_cm,
        })
    }
}

impl ApiSendable for Vec<ProcessInfo> {
    fn get_route(&self) -> &str {
        "/v1/proc"
    }

    fn to_json(&self) -> serde_json::Value {
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
    base_url: &str,
    data: &T,
) -> Result<(), Box<dyn Error>> {
    let url = format!("{}{}", base_url, data.get_route());
    debug!(url);
    let json_data = data.to_json();

    let response = client.post(&url).json(&json_data).send().await?;

    if response.status().is_success() {
        debug!("Data sent successfully to {}", url);
        Ok(())
    } else {
        Err(format!("API request failed with status: {}", response.status()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::mock;
    use mockito::server_url;
    use reqwest::Client;

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
        let base_url = server_url();

        let key_logger = KeyLogger {
            left_clicks: 100,
            right_clicks: 50,
            middle_clicks: 10,
            keys_pressed: 1000,
            pixels_moved: 5000.5,
            mouse_moved_cm: 200.0,
            ..Default::default()
        };

        let _ = send_to_api(&client, &base_url, &key_logger).await;
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
        let base_url = server_url();

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

        let _ = send_to_api(&client, &base_url, &process_info).await;
        m.assert();
    }

    #[tokio::test]
    async fn test_api_error_response() {
        let m = mock("POST", "/v1/keys").with_status(500).create();

        let client = Client::new();
        let base_url = server_url();

        let key_logger = KeyLogger {
            left_clicks: 100,
            right_clicks: 50,
            middle_clicks: 10,
            keys_pressed: 1000,
            pixels_moved: 5000.5,
            mouse_moved_cm: 200.0,
            ..Default::default()
        };

        let _ = send_to_api(&client, &base_url, &key_logger).await;
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
