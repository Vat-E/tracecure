//! Timestamp rendering. Pure, so it is testable off-wasm: the caller passes
//! the viewer's UTC offset and the current time in.

use chrono::{DateTime, Duration, FixedOffset, Utc};

use crate::state::TimestampMode;

fn zone(offset_secs: i32) -> FixedOffset {
    FixedOffset::east_opt(offset_secs).unwrap_or_else(|| FixedOffset::east_opt(0).expect("zero"))
}

/// The row's time column, in the viewer's zone.
///
/// A record from today shows a millisecond time. Anything older shows its
/// date instead of the milliseconds: on a multi-day window a bare `10:59:32`
/// says nothing about which day, and the column has room for one or the other.
#[must_use]
pub fn row_time(
    ts: &DateTime<Utc>,
    mode: TimestampMode,
    offset_secs: i32,
    now: DateTime<Utc>,
) -> String {
    match mode {
        TimestampMode::Absolute => {
            let z = zone(offset_secs);
            let local = ts.with_timezone(&z);
            if local.date_naive() == now.with_timezone(&z).date_naive() {
                local.format("%H:%M:%S%.3f").to_string()
            } else {
                local.format("%m-%d %H:%M:%S").to_string()
            }
        }
        TimestampMode::Relative => relative(now.signed_duration_since(*ts)),
    }
}

fn relative(d: Duration) -> String {
    let s = d.num_seconds().max(0);
    if s < 60 {
        format!("{s}s ago")
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else if s < 86_400 {
        format!("{}h ago", s / 3600)
    } else {
        format!("{}d ago", s / 86_400)
    }
}

/// Full local timestamp for the detail pane: date, milliseconds and zone.
#[must_use]
pub fn full_local(ts: &DateTime<Utc>, offset_secs: i32) -> String {
    format!(
        "{} {}",
        ts.with_timezone(&zone(offset_secs))
            .format("%Y-%m-%d %H:%M:%S%.3f"),
        zone_label(offset_secs)
    )
}

/// `UTC`, `UTC+1`, `UTC-3:30`.
#[must_use]
pub fn zone_label(offset_secs: i32) -> String {
    if offset_secs == 0 {
        return "UTC".to_string();
    }
    let sign = if offset_secs < 0 { '-' } else { '+' };
    let mins = offset_secs.abs() / 60;
    let (h, m) = (mins / 60, mins % 60);
    if m == 0 {
        format!("UTC{sign}{h}")
    } else {
        format!("UTC{sign}{h}:{m:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(d: u32, h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, d, h, m, s).unwrap()
    }

    #[test]
    fn todays_record_shows_local_time_with_millis() {
        let ts = at(10, 10, 59, 32) + Duration::milliseconds(473);
        let now = at(10, 11, 0, 0);
        assert_eq!(
            row_time(&ts, TimestampMode::Absolute, 3600, now),
            "11:59:32.473"
        );
    }

    #[test]
    fn an_older_record_shows_its_date() {
        let ts = at(8, 22, 15, 1);
        let now = at(10, 11, 0, 0);
        assert_eq!(
            row_time(&ts, TimestampMode::Absolute, 0, now),
            "11-08 22:15:01"
        );
    }

    #[test]
    fn today_is_decided_in_the_viewers_zone_not_utc() {
        // 23:30 UTC on the 9th is already the 10th at UTC+1.
        let ts = at(9, 23, 30, 0);
        let now = at(10, 8, 0, 0);
        assert_eq!(
            row_time(&ts, TimestampMode::Absolute, 3600, now),
            "00:30:00.000"
        );
        assert_eq!(
            row_time(&ts, TimestampMode::Absolute, 0, now),
            "11-09 23:30:00"
        );
    }

    #[test]
    fn relative_buckets() {
        let now = at(10, 12, 0, 0);
        let rel = |ts| row_time(&ts, TimestampMode::Relative, 0, now);
        assert_eq!(rel(at(10, 11, 59, 58)), "2s ago");
        assert_eq!(rel(at(10, 11, 57, 0)), "3m ago");
        assert_eq!(rel(at(10, 9, 0, 0)), "3h ago");
        assert_eq!(rel(at(8, 12, 0, 0)), "2d ago");
        // A record stamped slightly in the future must not go negative.
        assert_eq!(rel(at(10, 12, 0, 5)), "0s ago");
    }

    #[test]
    fn zone_labels() {
        assert_eq!(zone_label(0), "UTC");
        assert_eq!(zone_label(3600), "UTC+1");
        assert_eq!(zone_label(-10_800), "UTC-3");
        assert_eq!(zone_label(19_800), "UTC+5:30");
        assert_eq!(zone_label(-12_600), "UTC-3:30");
    }

    #[test]
    fn full_local_carries_date_and_zone() {
        let ts = at(10, 10, 59, 32) + Duration::milliseconds(473);
        assert_eq!(full_local(&ts, 3600), "2026-11-10 11:59:32.473 UTC+1");
    }
}
