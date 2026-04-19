//! Shared runtime types and helpers used across platforms.
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tokio::time::Duration;

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Datelike, FixedOffset, Offset, TimeZone, Timelike, Utc};

use tracing::*;

use std::env;
use std::io::{self};
use std::path::PathBuf;

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InputBucketKey {
    source_id: i64,
    bucket_start_utc: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FocusBucketKey {
    source_id: i64,
    bucket_start_utc: DateTime<Utc>,
    window_title: String,
    window_class: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BucketMetadata {
    pub bucket_start_utc: DateTime<Utc>,
    pub bucket_end_utc: DateTime<Utc>,
    pub local_date: String,
    pub local_hour: u32,
    pub timezone_offset_minutes: i32,
    pub granularity_minutes: u32,
}

#[derive(Debug, Default)]
pub struct InputBucketBuffer {
    source_id: i64,
    granularity_minutes: u32,
    buckets: HashMap<InputBucketKey, InputBucketRecord>,
}

impl InputBucketBuffer {
    pub fn new(source_id: i64, granularity_minutes: u32) -> Self {
        Self {
            source_id,
            granularity_minutes,
            buckets: HashMap::new(),
        }
    }

    fn bucket_mut(&mut self, at: DateTime<Utc>) -> &mut InputBucketRecord {
        let meta = bucket_metadata(at, self.granularity_minutes);
        let key = InputBucketKey {
            source_id: self.source_id,
            bucket_start_utc: meta.bucket_start_utc,
        };

        self.buckets
            .entry(key)
            .or_insert_with(|| InputBucketRecord {
                source_id: self.source_id,
                bucket_start_utc: meta.bucket_start_utc,
                bucket_end_utc: meta.bucket_end_utc,
                local_date: meta.local_date,
                local_hour: meta.local_hour,
                timezone_offset_minutes: meta.timezone_offset_minutes,
                granularity_minutes: meta.granularity_minutes,
                left_clicks: 0,
                right_clicks: 0,
                middle_clicks: 0,
                key_presses: 0,
                mouse_distance_cm: 0.0,
                scroll_vertical_cm: 0.0,
                scroll_horizontal_cm: 0.0,
            })
    }

    pub fn record_key_press(&mut self, at: DateTime<Utc>) {
        self.bucket_mut(at).key_presses += 1;
    }

    pub fn record_left_click(&mut self, at: DateTime<Utc>) {
        self.bucket_mut(at).left_clicks += 1;
    }

    pub fn record_right_click(&mut self, at: DateTime<Utc>) {
        self.bucket_mut(at).right_clicks += 1;
    }

    pub fn record_middle_click(&mut self, at: DateTime<Utc>) {
        self.bucket_mut(at).middle_clicks += 1;
    }

    pub fn record_mouse_distance_cm(&mut self, at: DateTime<Utc>, distance_cm: f64) {
        self.bucket_mut(at).mouse_distance_cm += distance_cm;
    }

    pub fn record_vertical_scroll_cm(&mut self, at: DateTime<Utc>, distance_cm: f64) {
        self.bucket_mut(at).scroll_vertical_cm += distance_cm;
    }

    pub fn record_horizontal_scroll_cm(&mut self, at: DateTime<Utc>, distance_cm: f64) {
        self.bucket_mut(at).scroll_horizontal_cm += distance_cm;
    }

    pub fn drain(&mut self) -> Vec<InputBucketRecord> {
        let mut rows = self.buckets.drain().map(|(_, row)| row).collect::<Vec<_>>();
        rows.sort_by_key(|row| row.bucket_start_utc);
        rows
    }
}

#[derive(Debug, Default)]
pub struct FocusBucketBuffer {
    source_id: i64,
    granularity_minutes: u32,
    buckets: HashMap<FocusBucketKey, FocusBucketRecord>,
}

impl FocusBucketBuffer {
    pub fn new(source_id: i64, granularity_minutes: u32) -> Self {
        Self {
            source_id,
            granularity_minutes,
            buckets: HashMap::new(),
        }
    }

    pub fn record_interval(&mut self, window: &Window, start: DateTime<Utc>, end: DateTime<Utc>) {
        if end <= start {
            return;
        }

        let mut cursor = start;
        while cursor < end {
            let meta = bucket_metadata(cursor, self.granularity_minutes);
            let segment_end = end.min(meta.bucket_end_utc);
            let seconds = (segment_end - cursor).num_seconds() as u64;

            if seconds > 0 {
                let key = FocusBucketKey {
                    source_id: self.source_id,
                    bucket_start_utc: meta.bucket_start_utc,
                    window_title: window.name.clone(),
                    window_class: window.class.clone(),
                };

                let record = self
                    .buckets
                    .entry(key)
                    .or_insert_with(|| FocusBucketRecord {
                        source_id: self.source_id,
                        bucket_start_utc: meta.bucket_start_utc,
                        bucket_end_utc: meta.bucket_end_utc,
                        local_date: meta.local_date,
                        local_hour: meta.local_hour,
                        timezone_offset_minutes: meta.timezone_offset_minutes,
                        app_identifier: window.app_identifier(),
                        window_title: window.name.clone(),
                        window_class: window.class.clone(),
                        focus_seconds: 0,
                    });

                record.focus_seconds += seconds;
            }

            cursor = segment_end;
        }
    }

    pub fn drain(&mut self) -> Vec<FocusBucketRecord> {
        let mut rows = self.buckets.drain().map(|(_, row)| row).collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            left.bucket_start_utc
                .cmp(&right.bucket_start_utc)
                .then_with(|| left.window_class.cmp(&right.window_class))
                .then_with(|| left.window_title.cmp(&right.window_title))
        });
        rows
    }
}

#[derive(Debug)]
pub struct ProcessTracker {
    pending: FocusBucketBuffer,
    active_window: Option<Window>,
    active_since_utc: Option<DateTime<Utc>>,
}

impl ProcessTracker {
    pub fn new(source_id: i64, granularity_minutes: u32) -> Self {
        Self {
            pending: FocusBucketBuffer::new(source_id, granularity_minutes),
            active_window: None,
            active_since_utc: None,
        }
    }

    pub fn switch_window(&mut self, window: Window, now: DateTime<Utc>) {
        self.record_active_until(now);
        self.active_window = Some(window);
        self.active_since_utc = Some(now);
    }

    pub fn pause(&mut self, now: DateTime<Utc>) {
        self.record_active_until(now);
        self.active_since_utc = None;
    }

    pub fn resume(&mut self, now: DateTime<Utc>) {
        if self.active_window.is_some() && self.active_since_utc.is_none() {
            self.active_since_utc = Some(now);
        }
    }

    pub fn record_active_until(&mut self, now: DateTime<Utc>) {
        let Some(window) = self.active_window.as_ref() else {
            return;
        };
        let Some(start) = self.active_since_utc else {
            return;
        };

        self.pending.record_interval(window, start, now);
        self.active_since_utc = Some(now);
    }

    pub fn clear_focus(&mut self, now: DateTime<Utc>) {
        self.record_active_until(now);
        self.active_window = None;
        self.active_since_utc = None;
    }

    #[allow(dead_code)]
    pub fn current_window_name(&self) -> Option<&str> {
        self.active_window
            .as_ref()
            .map(|window| window.name.as_str())
    }

    #[allow(dead_code)]
    pub fn current_window_class(&self) -> Option<&str> {
        self.active_window
            .as_ref()
            .map(|window| window.class.as_str())
    }

    pub fn drain_pending(&mut self) -> Vec<FocusBucketRecord> {
        self.pending.drain()
    }
}

/// Spawns a new asynchronous task that sends a message on a channel at a regular interval.
pub fn spawn_ticker<T>(tx: mpsc::Sender<T>, duration: Duration, event_to_send: T) -> JoinHandle<()>
where
    T: Clone + Send + 'static,
{
    let join_handle = tokio::spawn(async move {
        let mut interval = interval(duration);
        interval.tick().await;
        loop {
            interval.tick().await;
            if tx.send(event_to_send.clone()).await.is_err() {
                error!("Ticker channel closed. Shutting down ticker task");
                break;
            }
        }
    });

    join_handle
}

pub fn bucket_metadata(at: DateTime<Utc>, granularity_minutes: u32) -> BucketMetadata {
    let granularity_minutes = granularity_minutes.max(1);
    let local = at.with_timezone(&chrono::Local);
    let offset = local.offset().fix();
    bucket_metadata_with_offset(at, granularity_minutes, offset)
}

pub fn euclidean_distance(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}

pub fn counts_to_centimeters(counts: f64, dpi: f64) -> f64 {
    if dpi <= 0.0 {
        return 0.0;
    }

    counts / dpi * 2.54
}

pub fn relative_counts_to_centimeters(dx: f64, dy: f64, dpi: f64) -> f64 {
    counts_to_centimeters(euclidean_distance(dx, dy), dpi)
}

#[cfg(target_os = "windows")]
pub fn millimeters_to_centimeters(dx_mm: f64, dy_mm: f64) -> f64 {
    euclidean_distance(dx_mm, dy_mm) / 10.0
}

pub fn scroll_steps_to_centimeters(steps: f64) -> f64 {
    steps.abs() * ASSUMED_CM_PER_SCROLL_STEP
}

fn bucket_metadata_with_offset(
    at: DateTime<Utc>,
    granularity_minutes: u32,
    offset: FixedOffset,
) -> BucketMetadata {
    let local = at.with_timezone(&offset);
    let total_minutes = (local.hour() * 60 + local.minute()) as i64;
    let granularity = granularity_minutes as i64;
    let floored_minutes = total_minutes - (total_minutes % granularity);
    let start_local = offset
        .with_ymd_and_hms(
            local.year(),
            local.month(),
            local.day(),
            (floored_minutes / 60) as u32,
            (floored_minutes % 60) as u32,
            0,
        )
        .single()
        .expect("bucket start should be representable");
    let end_local = start_local + chrono::Duration::minutes(granularity);

    BucketMetadata {
        bucket_start_utc: start_local.with_timezone(&Utc),
        bucket_end_utc: end_local.with_timezone(&Utc),
        local_date: start_local.format("%Y-%m-%d").to_string(),
        local_hour: start_local.hour(),
        timezone_offset_minutes: offset.local_minus_utc() / 60,
        granularity_minutes,
    }
}

/// Returns a platform-specific path for storing program-related files and ensures the directory exists.
pub fn program_data_dir() -> io::Result<PathBuf> {
    if let Ok(path) = env::var("LIFE_MONITOR_DATA_DIR") {
        let path = PathBuf::from(path);
        std::fs::create_dir_all(&path)?;
        return Ok(path);
    }

    #[cfg(test)]
    {
        if env::var_os("LIFE_MONITOR_DATA_DIR").is_none() {
            let path = std::env::temp_dir().join("life_monitor_test_data");
            std::fs::create_dir_all(&path)?;
            return Ok(path);
        }
    }

    let base_dir = if cfg!(target_os = "windows") {
        env::var("LOCALAPPDATA").map(PathBuf::from).map_err(|_| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "LOCALAPPDATA environment variable not set",
            )
        })?
    } else if cfg!(target_os = "linux") {
        let home = env::var("HOME").map_err(|_| {
            io::Error::new(io::ErrorKind::NotFound, "HOME environment variable not set")
        })?;
        let mut path = PathBuf::from(home);
        path.push(".local");
        path.push("share");
        path
    } else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unsupported operating system",
        ));
    };

    let path = base_dir.join("life_monitor");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_metadata_tracks_local_boundaries() {
        let at = Utc.with_ymd_and_hms(2026, 4, 18, 13, 37, 42).unwrap();
        let offset = FixedOffset::west_opt(3 * 3600).unwrap();

        let bucket = bucket_metadata_with_offset(at, 15, offset);

        assert_eq!(
            bucket.bucket_start_utc,
            Utc.with_ymd_and_hms(2026, 4, 18, 13, 30, 0).unwrap()
        );
        assert_eq!(
            bucket.bucket_end_utc,
            Utc.with_ymd_and_hms(2026, 4, 18, 13, 45, 0).unwrap()
        );
        assert_eq!(bucket.local_date, "2026-04-18");
        assert_eq!(bucket.local_hour, 10);
        assert_eq!(bucket.timezone_offset_minutes, -180);
    }

    #[test]
    fn input_buffer_aggregates_events_into_matching_bucket() {
        let mut buffer = InputBucketBuffer::new(DEFAULT_SOURCE_ID, 15);
        let at = Utc.with_ymd_and_hms(2026, 4, 18, 13, 4, 0).unwrap();

        buffer.record_key_press(at);
        buffer.record_left_click(at);
        buffer.record_mouse_distance_cm(at, 2.5);
        buffer.record_vertical_scroll_cm(at, 0.8);

        let rows = buffer.drain();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key_presses, 1);
        assert_eq!(rows[0].left_clicks, 1);
        assert_eq!(rows[0].mouse_distance_cm, 2.5);
        assert_eq!(rows[0].scroll_vertical_cm, 0.8);
        assert_eq!(rows[0].granularity_minutes, 15);
    }

    #[test]
    fn focus_buffer_splits_intervals_across_bucket_boundaries() {
        let mut buffer = FocusBucketBuffer::new(DEFAULT_SOURCE_ID, 15);
        let window = Window {
            name: "Editor".to_string(),
            class: "nvim".to_string(),
        };
        let start = Utc.with_ymd_and_hms(2026, 4, 18, 12, 58, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 4, 18, 13, 4, 30).unwrap();

        buffer.record_interval(&window, start, end);

        let rows = buffer.drain();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].focus_seconds, 120);
        assert_eq!(rows[1].focus_seconds, 270);
        assert_eq!(rows[0].window_title, "Editor");
        assert_eq!(rows[1].window_class, "nvim");
    }

    #[test]
    fn relative_counts_to_centimeters_uses_euclidean_distance() {
        let distance_cm = relative_counts_to_centimeters(3.0, 4.0, 800.0);
        assert!((distance_cm - (5.0 / 800.0 * 2.54)).abs() < 1e-6);
    }

    #[test]
    fn scroll_steps_to_centimeters_uses_absolute_step_count() {
        assert!((scroll_steps_to_centimeters(-2.0) - 0.8).abs() < 1e-6);
    }
}
