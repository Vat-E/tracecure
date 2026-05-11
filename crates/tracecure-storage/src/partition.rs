//! Partition identity: floor a timestamp to a partition boundary and format
//! it as a filesystem-safe label.

use std::time::Duration;

use chrono::{DateTime, DurationRound, TimeDelta, Utc};

#[derive(Debug, Clone, Copy)]
pub struct PartitionConfig {
    /// Partition granularity. 1 h default.
    pub granularity: Duration,
    /// Minimum time past a partition's end before it is eligible for
    /// compaction. 30 min default.
    pub grace: Duration,
}

impl Default for PartitionConfig {
    fn default() -> Self {
        Self {
            granularity: Duration::from_secs(3600),
            grace: Duration::from_secs(30 * 60),
        }
    }
}

/// Floor `ts` to the previous partition boundary.
#[must_use]
pub fn partition_id(ts: DateTime<Utc>, granularity: Duration) -> DateTime<Utc> {
    let delta = TimeDelta::from_std(granularity).unwrap_or(TimeDelta::seconds(1));
    ts.duration_trunc(delta).unwrap_or(ts)
}

/// Filesystem-safe label for a partition. Uses RFC3339 minus the timezone
/// suffix, swapping colons for underscores when granularity is sub-minute
/// (Windows forbids colons in paths).
#[must_use]
pub fn partition_label(id: DateTime<Utc>, granularity: Duration) -> String {
    let g = granularity.as_secs();
    if g >= 3600 {
        id.format("%Y-%m-%dT%H").to_string()
    } else if g >= 60 {
        id.format("%Y-%m-%dT%H_%M").to_string()
    } else {
        id.format("%Y-%m-%dT%H_%M_%S%.3f").to_string()
    }
}

/// Inclusive partition end = partition start + granularity.
#[must_use]
pub fn partition_end(id: DateTime<Utc>, granularity: Duration) -> DateTime<Utc> {
    id + TimeDelta::from_std(granularity).unwrap_or(TimeDelta::seconds(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 23, h, m, s).unwrap()
    }

    #[test]
    fn record_maps_to_expected_hour_partition() {
        let id = partition_id(t(12, 34, 56), Duration::from_secs(3600));
        assert_eq!(id, t(12, 0, 0));
    }

    #[test]
    fn granularity_1s_rounds_correctly() {
        let id = partition_id(t(12, 34, 56), Duration::from_secs(1));
        assert_eq!(id, t(12, 34, 56));
    }

    #[test]
    fn label_for_1h_is_hour_precision() {
        let id = t(12, 0, 0);
        assert_eq!(
            partition_label(id, Duration::from_secs(3600)),
            "2026-04-23T12"
        );
    }

    #[test]
    fn label_for_1min_uses_underscores_not_colons() {
        let id = t(12, 34, 0);
        let label = partition_label(id, Duration::from_secs(60));
        assert!(
            !label.contains(':'),
            "minute label should be Windows-path-safe: {label}"
        );
        assert_eq!(label, "2026-04-23T12_34");
    }

    #[test]
    fn label_for_sub_minute_uses_underscores_not_colons() {
        let id = Utc.with_ymd_and_hms(2026, 4, 23, 12, 34, 56).unwrap()
            + chrono::Duration::milliseconds(100);
        let label = partition_label(id, Duration::from_millis(100));
        assert!(
            !label.contains(':'),
            "sub-minute label should be Windows-path-safe: {label}"
        );
        assert!(label.starts_with("2026-04-23T12_34_56"));
    }

    #[test]
    fn partition_end_is_start_plus_granularity() {
        let id = t(12, 0, 0);
        let end = partition_end(id, Duration::from_secs(3600));
        assert_eq!(end, t(13, 0, 0));
    }

    #[test]
    fn records_in_same_partition_share_id() {
        let g = Duration::from_secs(3600);
        let a = partition_id(t(12, 0, 1), g);
        let b = partition_id(t(12, 59, 59), g);
        assert_eq!(a, b);
    }

    #[test]
    fn records_across_boundary_get_different_ids() {
        let g = Duration::from_secs(3600);
        let a = partition_id(t(12, 59, 59), g);
        let b = partition_id(t(13, 0, 1), g);
        assert_ne!(a, b);
    }
}
