use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::buckets::bucket_metadata;
use super::types::InputBucketRecord;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InputBucketKey {
    source_id: i64,
    bucket_start_utc: DateTime<Utc>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::DEFAULT_SOURCE_ID;
    use chrono::TimeZone;

    /// Verifies that buffered input events aggregate into one row per time bucket by mixing
    /// events that stay in the same bucket with one event that crosses into the next bucket.
    #[test]
    fn input_buffer_aggregates_events_into_matching_bucket() {
        let mut buffer = InputBucketBuffer::new(DEFAULT_SOURCE_ID, 15);
        let start = Utc.with_ymd_and_hms(2026, 4, 18, 12, 0, 5).unwrap();
        let same_bucket = Utc.with_ymd_and_hms(2026, 4, 18, 12, 14, 59).unwrap();
        let next_bucket = Utc.with_ymd_and_hms(2026, 4, 18, 12, 15, 1).unwrap();

        buffer.record_key_press(start);
        buffer.record_left_click(start);
        buffer.record_mouse_distance_cm(start, 1.25);
        buffer.record_vertical_scroll_cm(same_bucket, 0.5);
        buffer.record_horizontal_scroll_cm(next_bucket, 0.4);

        let rows = buffer.drain();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key_presses, 1);
        assert_eq!(rows[0].left_clicks, 1);
        assert!((rows[0].mouse_distance_cm - 1.25).abs() < 1e-6);
        assert!((rows[0].scroll_vertical_cm - 0.5).abs() < 1e-6);
        assert_eq!(rows[1].scroll_horizontal_cm, 0.4);
    }
}
