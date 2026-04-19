#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityType {
    Source,
    InputBucket,
    FocusBucket,
}

impl EntityType {
    pub fn as_str(self) -> &'static str {
        match self {
            EntityType::Source => "source",
            EntityType::InputBucket => "input_bucket",
            EntityType::FocusBucket => "focus_bucket",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceChange {
    pub source_uuid: String,
    pub source_name: String,
    pub platform: String,
    pub created_at_utc: String,
}

impl SourceChange {
    pub fn entity_key(&self) -> String {
        format!("source:{}", self.source_uuid)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputBucketChange {
    pub source_uuid: String,
    pub bucket_start_utc: String,
    pub bucket_end_utc: String,
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

impl InputBucketChange {
    pub fn entity_key(&self) -> String {
        format!(
            "input:{}:{}:{}",
            self.source_uuid, self.bucket_start_utc, self.granularity_minutes
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FocusBucketChange {
    pub source_uuid: String,
    pub bucket_start_utc: String,
    pub bucket_end_utc: String,
    pub local_date: String,
    pub local_hour: u32,
    pub timezone_offset_minutes: i32,
    pub app_identifier: String,
    pub window_title: String,
    pub window_class: String,
    pub focus_seconds: u64,
}

impl FocusBucketChange {
    pub fn entity_key(&self) -> String {
        format!(
            "focus:{}:{}:{}:{}",
            self.source_uuid, self.bucket_start_utc, self.window_title, self.window_class
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChangePayload {
    Source(SourceChange),
    InputBucket(InputBucketChange),
    FocusBucket(FocusBucketChange),
}

impl ChangePayload {
    pub fn source_uuid(&self) -> &str {
        match self {
            ChangePayload::Source(payload) => &payload.source_uuid,
            ChangePayload::InputBucket(payload) => &payload.source_uuid,
            ChangePayload::FocusBucket(payload) => &payload.source_uuid,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutboxEntry {
    pub id: i64,
    pub batch_uuid: Option<String>,
    pub entity_type: EntityType,
    pub entity_key: String,
    pub source_uuid: String,
    pub payload: ChangePayload,
    pub created_at_utc: String,
    pub sent_at_utc: Option<String>,
    pub attempt_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PushBatch {
    pub batch_uuid: String,
    pub source_uuid: String,
    pub source_changes: Vec<SourceChange>,
    pub input_changes: Vec<InputBucketChange>,
    pub focus_changes: Vec<FocusBucketChange>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PushAck {
    pub applied_revision: i64,
    pub remote_head_revision: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteChange {
    pub revision: i64,
    pub payload: ChangePayload,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PullResponse {
    pub remote_head_revision: i64,
    pub changes: Vec<RemoteChange>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteStatus {
    pub remote_head_revision: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncStateRecord {
    pub own_source_uuid: String,
    pub remote_url: String,
    pub last_pulled_revision: i64,
    pub last_pushed_batch_uuid: Option<String>,
    pub last_push_at_utc: Option<String>,
    pub last_pull_at_utc: Option<String>,
    pub last_sync_error: Option<String>,
    pub last_sync_error_at_utc: Option<String>,
    pub remote_head_revision: Option<i64>,
    pub sync_enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncStatusSnapshot {
    pub sync_enabled: bool,
    pub remote_url: String,
    pub own_source_uuid: String,
    pub last_push_at_utc: Option<String>,
    pub last_pull_at_utc: Option<String>,
    pub last_push_age_seconds: Option<i64>,
    pub last_pull_age_seconds: Option<i64>,
    pub remote_head_revision: Option<i64>,
    pub last_pulled_revision: i64,
    pub pending_outbox_count: u64,
    pub last_sync_error: Option<String>,
    pub last_sync_error_at_utc: Option<String>,
    pub is_caught_up: Option<bool>,
}
