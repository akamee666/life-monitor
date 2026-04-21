use chrono::{DateTime, Utc};
use std::collections::HashSet;

pub const DEFAULT_SOURCE_ID: i64 = 1;
pub const DEFAULT_BUCKET_MINUTES: i64 = 15;
pub const DEFAULT_MOUSE_DPI: u32 = 800;
pub const ASSUMED_CM_PER_SCROLL_STEP: f64 = 0.4;

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum Signals {
    Tick,
    DbUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Window {
    pub name: String,
    pub class: String,
}

impl Window {
    pub fn app_identifier(&self) -> String {
        self.class.trim().to_ascii_lowercase()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputBucketRecord {
    pub source_id: i64,
    pub bucket_start_utc: DateTime<Utc>,
    pub bucket_end_utc: DateTime<Utc>,
    pub local_date: String,
    pub local_hour: u32,
    pub timezone_offset_minutes: i32,
    pub granularity_minutes: u32,
    pub left_clicks: u64,
    pub right_clicks: u64,
    pub middle_clicks: u64,
    pub key_presses: u64,
    pub mouse_distance_cm: f64,
    pub scroll_vertical_cm: f64,
    pub scroll_horizontal_cm: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FocusBucketRecord {
    pub source_id: i64,
    pub bucket_start_utc: DateTime<Utc>,
    pub bucket_end_utc: DateTime<Utc>,
    pub local_date: String,
    pub local_hour: u32,
    pub timezone_offset_minutes: i32,
    pub app_identifier: String,
    pub window_title: String,
    pub window_class: String,
    pub focus_seconds: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceInfo {
    pub id: i64,
    pub source_uuid: String,
    pub source_name: String,
    pub platform: String,
}

#[derive(Debug, Default, Clone, PartialEq)]
#[allow(dead_code)]
pub struct InputLogger {
    #[cfg(target_os = "windows")]
    pub w: WindowsSpecific,
    pub left_clicks: u64,
    pub right_clicks: u64,
    pub middle_clicks: u64,
    pub key_presses: u64,
    pub pixels_traveled: u64,
    pub cm_traveled: f64,
    pub mouse_dpi: u64,
    pub vertical_scroll_clicks: u64,
    pub horizontal_scroll_clicks: u64,
    pub vertical_scroll_cm: f64,
    pub horizontal_scroll_cm: f64,
}

#[derive(Debug, Default, Clone, PartialEq)]
#[allow(dead_code)]
pub struct WindowsSpecific {
    pub pressed_keys_state: HashSet<u16>,
    pub screen_width_mm: f64,
    pub screen_height_mm: f64,
    pub last_abs_x: Option<i32>,
    pub last_abs_y: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::Window;

    #[test]
    fn app_identifier_is_trimmed_and_lowercased() {
        let window = Window {
            name: "Ghostty".to_string(),
            class: "  Com.Mitchellh.Ghostty  ".to_string(),
        };

        assert_eq!(window.app_identifier(), "com.mitchellh.ghostty");
    }
}
