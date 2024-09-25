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
