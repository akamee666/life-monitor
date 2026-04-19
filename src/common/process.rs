use chrono::{DateTime, Utc};

use super::focus::FocusBucketBuffer;
use super::types::{FocusBucketRecord, Window};

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
