//! Retention by time: which records are old enough to delete.
//!
//! A rule is a maximum age. There is one optional default rule and any number
//! of per-stream rules keyed by `stream_id`; a stream with its own rule
//! ignores the default. With no rules at all nothing is ever deleted, which
//! is the default — a store must never drop data it was not told to drop.
//!
//! Retention is **partition-aligned**. A record is not deleted the moment it
//! passes its age; a stream's records are deleted from a partition once the
//! *whole partition* is older than that stream's rule. So data lives up to
//! one partition (an hour, by default) longer than the rule says, plus the
//! compactor's grace and tick. In exchange each partition is rewritten at
//! most once per rule instead of once per tick as its records age out one by
//! one. See ADR-003.
//!
//! This module only answers questions; `compactor.rs` does the deleting.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetentionConfig {
    /// Maximum age for streams without a rule of their own. `None` = such
    /// streams are kept forever.
    pub default: Option<Duration>,
    /// Maximum age per `stream_id`. Overrides `default` for that stream, in
    /// either direction: shorter to drop a noisy stream early, longer to
    /// keep an important one past the default.
    pub per_stream: BTreeMap<String, Duration>,
}

/// Which rules had fully expired for a partition when a segment was last
/// rewritten. Comparing it with the current answer says whether the segment
/// needs rewriting again, without reading the segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiredRules {
    /// The default rule has expired for this partition.
    pub default: bool,
    /// Per-stream rules that have expired for this partition.
    pub streams: BTreeSet<String>,
}

impl RetentionConfig {
    /// Are there any rules at all?
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.default.is_some() || !self.per_stream.is_empty()
    }

    /// The maximum age that applies to `stream_id`. `None` = keep forever.
    #[must_use]
    pub fn max_age(&self, stream_id: &str) -> Option<Duration> {
        self.per_stream.get(stream_id).copied().or(self.default)
    }

    /// Should a record of `stream_id` be dropped from a partition that ended
    /// at `partition_end`?
    #[must_use]
    pub fn drops(&self, stream_id: &str, partition_end: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        self.max_age(stream_id)
            .is_some_and(|age| expired(partition_end, age, now))
    }

    /// The rules that have expired for a partition, or `None` if none have —
    /// i.e. nothing in it is due for deletion.
    #[must_use]
    pub fn expired_rules(
        &self,
        partition_end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Option<ExpiredRules> {
        let default = self
            .default
            .is_some_and(|age| expired(partition_end, age, now));
        let streams: BTreeSet<String> = self
            .per_stream
            .iter()
            .filter(|(_, age)| expired(partition_end, **age, now))
            .map(|(stream, _)| stream.clone())
            .collect();
        (default || !streams.is_empty()).then_some(ExpiredRules { default, streams })
    }

    /// Has *every* record in the partition expired, whatever stream it
    /// belongs to? Then its segments can be deleted without reading them.
    ///
    /// That needs a default rule (otherwise a stream with no rule is kept
    /// forever and could be in there) and every rule, the longest included,
    /// to have expired.
    #[must_use]
    pub fn fully_expired(&self, partition_end: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        let Some(default) = self.default else {
            return false;
        };
        let longest = self
            .per_stream
            .values()
            .copied()
            .fold(default, Duration::max);
        expired(partition_end, longest, now)
    }
}

/// Is a partition that ended at `partition_end` older than `age`?
fn expired(partition_end: DateTime<Utc>, age: Duration, now: DateTime<Utc>) -> bool {
    // An age too large to represent can never be reached.
    TimeDelta::from_std(age).is_ok_and(|age| {
        partition_end
            .checked_add_signed(age)
            .is_some_and(|deadline| deadline <= now)
    })
}

/// Parse a duration written as a whole number and a unit: `90s`, `15m`,
/// `12h`, `30d`, `8w`.
///
/// # Errors
///
/// Returns a message naming what was wrong: an empty string, a missing or
/// unknown unit, a number that is not a positive integer, or an overflow.
pub fn parse_duration(input: &str) -> Result<Duration, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("empty duration".to_string());
    }
    let split = s
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| format!("\"{s}\" has no unit; use s, m, h, d or w (e.g. 30d)"))?;
    let (number, unit) = s.split_at(split);
    let n: u64 = number
        .parse()
        .map_err(|_| format!("\"{s}\" must start with a whole number (e.g. 30d)"))?;
    if n == 0 {
        return Err(format!(
            "\"{s}\" is zero; to keep nothing, do not ingest it"
        ));
    }
    let unit_secs: u64 = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        "w" => 604_800,
        other => {
            return Err(format!(
                "unknown unit \"{other}\" in \"{s}\"; use s, m, h, d or w"
            ))
        }
    };
    n.checked_mul(unit_secs)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("\"{s}\" is too large"))
}

/// Parse a per-stream rule written as `<stream_id>=<duration>`.
///
/// The split is at the **last** `=`, so a stream id may itself contain one.
///
/// # Errors
///
/// Returns a message if there is no `=`, the stream id is empty, or the
/// duration does not parse.
pub fn parse_stream_rule(input: &str) -> Result<(String, Duration), String> {
    let (stream, duration) = input
        .rsplit_once('=')
        .ok_or_else(|| format!("\"{input}\" is not <stream_id>=<duration> (e.g. api-prod=7d)"))?;
    let stream = stream.trim();
    if stream.is_empty() {
        return Err(format!("\"{input}\" has an empty stream id"));
    }
    Ok((stream.to_string(), parse_duration(duration)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const HOUR: Duration = Duration::from_secs(3600);
    const DAY: Duration = Duration::from_secs(86_400);

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, day, hour, 0, 0).unwrap()
    }

    fn cfg(default: Option<Duration>, streams: &[(&str, Duration)]) -> RetentionConfig {
        RetentionConfig {
            default,
            per_stream: streams
                .iter()
                .map(|(s, d)| ((*s).to_string(), *d))
                .collect(),
        }
    }

    #[test]
    fn no_rules_means_nothing_is_ever_dropped() {
        let c = RetentionConfig::default();
        assert!(!c.is_enabled());
        assert!(!c.drops("any", at(1, 0), at(30, 0)));
        assert_eq!(c.expired_rules(at(1, 0), at(30, 0)), None);
        assert!(!c.fully_expired(at(1, 0), at(30, 0)));
    }

    #[test]
    fn a_stream_rule_overrides_the_default_in_both_directions() {
        let c = cfg(Some(DAY * 7), &[("noisy", DAY), ("audit", DAY * 90)]);
        assert_eq!(c.max_age("noisy"), Some(DAY));
        assert_eq!(c.max_age("audit"), Some(DAY * 90));
        assert_eq!(c.max_age("anything-else"), Some(DAY * 7));
    }

    #[test]
    fn expiry_is_measured_from_the_partition_end() {
        let c = cfg(Some(DAY), &[]);
        let end = at(10, 12);
        // One second short of a day after the partition ended: still kept.
        assert!(!c.drops("s", end, end + TimeDelta::seconds(86_399)));
        // Exactly a day: dropped.
        assert!(c.drops("s", end, end + TimeDelta::days(1)));
    }

    #[test]
    fn a_stream_without_a_rule_is_kept_when_there_is_no_default() {
        let c = cfg(None, &[("noisy", HOUR)]);
        assert!(c.drops("noisy", at(1, 0), at(2, 0)));
        assert!(!c.drops("other", at(1, 0), at(30, 0)));
    }

    #[test]
    fn expired_rules_lists_exactly_the_rules_that_have_passed() {
        let c = cfg(Some(DAY * 7), &[("noisy", DAY), ("audit", DAY * 90)]);
        let end = at(1, 0);
        assert_eq!(c.expired_rules(end, end + TimeDelta::hours(12)), None);
        assert_eq!(
            c.expired_rules(end, end + TimeDelta::days(2)),
            Some(ExpiredRules {
                default: false,
                streams: BTreeSet::from(["noisy".to_string()]),
            })
        );
        assert_eq!(
            c.expired_rules(end, end + TimeDelta::days(8)),
            Some(ExpiredRules {
                default: true,
                streams: BTreeSet::from(["noisy".to_string()]),
            })
        );
    }

    #[test]
    fn fully_expired_waits_for_the_longest_rule() {
        let c = cfg(Some(DAY * 7), &[("audit", DAY * 90)]);
        let end = at(1, 0);
        // The default has passed, but `audit` may still be in there.
        assert!(!c.fully_expired(end, end + TimeDelta::days(8)));
        assert!(c.fully_expired(end, end + TimeDelta::days(90)));
    }

    #[test]
    fn fully_expired_is_never_true_without_a_default() {
        // A stream with no rule is kept forever and could be in any segment.
        let c = cfg(None, &[("noisy", HOUR)]);
        assert!(!c.fully_expired(at(1, 0), at(30, 0)));
    }

    #[test]
    fn an_unrepresentable_age_never_expires() {
        let c = cfg(Some(Duration::MAX), &[]);
        assert!(!c.drops("s", at(1, 0), at(30, 0)));
        assert!(!c.fully_expired(at(1, 0), at(30, 0)));
    }

    #[test]
    fn parses_durations_in_every_unit() {
        assert_eq!(parse_duration("90s"), Ok(Duration::from_secs(90)));
        assert_eq!(parse_duration("15m"), Ok(Duration::from_secs(900)));
        assert_eq!(parse_duration("12h"), Ok(Duration::from_secs(43_200)));
        assert_eq!(parse_duration("30d"), Ok(DAY * 30));
        assert_eq!(parse_duration(" 8w "), Ok(DAY * 56));
    }

    #[test]
    fn rejects_durations_it_cannot_read_and_says_why() {
        for bad in ["", "30", "d", "1.5d", "-3d", "30 d", "30days", "0d"] {
            assert!(parse_duration(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(parse_duration("99999999999999999999w").is_err());
        assert!(parse_duration("18446744073709551615w")
            .unwrap_err()
            .contains("too large"));
    }

    #[test]
    fn parses_a_stream_rule_splitting_at_the_last_equals() {
        assert_eq!(
            parse_stream_rule("api-prod=7d"),
            Ok(("api-prod".to_string(), DAY * 7))
        );
        assert_eq!(
            parse_stream_rule("docker/web=1h"),
            Ok(("docker/web".to_string(), HOUR))
        );
        assert_eq!(
            parse_stream_rule("env=prod/api=2d"),
            Ok(("env=prod/api".to_string(), DAY * 2))
        );
    }

    #[test]
    fn rejects_malformed_stream_rules() {
        assert!(parse_stream_rule("api-prod").is_err());
        assert!(parse_stream_rule("=7d").is_err());
        assert!(parse_stream_rule("api-prod=soon").is_err());
    }
}
