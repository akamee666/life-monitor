use chrono::{DateTime, Datelike, FixedOffset, Offset, TimeZone, Timelike, Utc};

#[derive(Debug, Clone, PartialEq)]
pub struct BucketMetadata {
    pub bucket_start_utc: DateTime<Utc>,
    pub bucket_end_utc: DateTime<Utc>,
    pub local_date: String,
    pub local_hour: u32,
    pub timezone_offset_minutes: i32,
    pub granularity_minutes: u32,
}

pub fn bucket_metadata(at: DateTime<Utc>, granularity_minutes: u32) -> BucketMetadata {
    let granularity_minutes = granularity_minutes.max(1);
    let local = at.with_timezone(&chrono::Local);
    let offset = local.offset().fix();
    bucket_metadata_with_offset(at, granularity_minutes, offset)
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Verifies that bucket metadata snaps timestamps to the correct local bucket edges
    /// by calling the offset-aware helper with a fixed timezone instead of relying on host time.
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
}
