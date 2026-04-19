//! Shared runtime types and helpers used across platforms.

mod buckets;
mod focus;
mod input;
mod motion;
mod paths;
mod process;
#[cfg(target_os = "linux")]
mod ticker;
mod types;

#[allow(unused_imports)]
pub use buckets::{bucket_metadata, BucketMetadata};
#[allow(unused_imports)]
pub use focus::FocusBucketBuffer;
pub use input::InputBucketBuffer;
#[cfg(target_os = "windows")]
pub use motion::millimeters_to_centimeters;
#[allow(unused_imports)]
pub use motion::{
    counts_to_centimeters, euclidean_distance, relative_counts_to_centimeters,
    scroll_steps_to_centimeters,
};
pub use paths::program_data_dir;
pub use process::ProcessTracker;
#[cfg(target_os = "linux")]
pub use ticker::spawn_ticker;
#[allow(unused_imports)]
pub use types::{
    FocusBucketRecord, InputBucketRecord, InputLogger, Signals, SourceInfo, Window,
    WindowsSpecific, ASSUMED_CM_PER_SCROLL_STEP, DEFAULT_BUCKET_MINUTES, DEFAULT_MOUSE_DPI,
    DEFAULT_SOURCE_ID,
};
