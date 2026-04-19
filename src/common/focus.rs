use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::buckets::bucket_metadata;
use super::types::{FocusBucketRecord, Window};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FocusBucketKey {
    source_id: i64,
    bucket_start_utc: DateTime<Utc>,
    window_title: String,
    window_class: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::DEFAULT_SOURCE_ID;
    use chrono::TimeZone;

    #[test]
    fn focus_buffer_splits_intervals_across_bucket_boundaries() {
        let mut buffer = FocusBucketBuffer::new(DEFAULT_SOURCE_ID, 15);
        let window = Window {
            name: "Editor".to_string(),
            class: "nvim".to_string(),
        };

        let start = Utc.with_ymd_and_hms(2026, 4, 18, 12, 14, 30).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 4, 18, 12, 15, 45).unwrap();
        buffer.record_interval(&window, start, end);

        let rows = buffer.drain();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].focus_seconds, 30);
        assert_eq!(rows[1].focus_seconds, 45);
        assert_eq!(rows[0].app_identifier, "nvim");
    }
}
