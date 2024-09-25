use life_monitor::api::*;
use life_monitor::{keylogger::KeyLogger, processinfo::ProcessInfo};
use serde_json::json;

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
                "mouse_moved_cm": 200,
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
            mouse_moved_cm: 200,
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
            mouse_moved_cm: 200,
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
            mouse_moved_cm: 200,
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
                "mouse_moved_cm": 200,
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
