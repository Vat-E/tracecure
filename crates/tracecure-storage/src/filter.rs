//! What a search is looking for, beyond its time range.
//!
//! A filter is a conjunction: every constraint that is set must hold. Within
//! one constraint the alternatives are a disjunction — `levels: [error,
//! fatal]` means either. An unset constraint (empty text, empty list) matches
//! everything, so `SearchFilter::default()` is "all records".
//!
//! This is deliberately the small, fixed set of equality tests the record's
//! own columns support cheaply. A query language belongs to M2; these are the
//! pushdowns it will keep.

use tracecure_core::{Level, Record};

/// A name to match exactly, or — written with a trailing `*` — by prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamePattern {
    Exact(String),
    Prefix(String),
}

impl NamePattern {
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.strip_suffix('*') {
            Some(prefix) => Self::Prefix(prefix.to_string()),
            None => Self::Exact(s.to_string()),
        }
    }

    #[must_use]
    pub fn matches(&self, value: &str) -> bool {
        match self {
            Self::Exact(name) => name == value,
            Self::Prefix(prefix) => value.starts_with(prefix.as_str()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFilter {
    /// Case-sensitive substring of `body`. Empty = any.
    pub text: String,
    /// Empty = any level.
    pub levels: Vec<Level>,
    /// Empty = any app.
    pub apps: Vec<NamePattern>,
    /// Empty = any host.
    pub hosts: Vec<NamePattern>,
    /// Empty = any `stream_id`.
    pub streams: Vec<NamePattern>,
}

impl SearchFilter {
    /// A filter on body text alone.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// Build a filter from the wire form: each constraint a comma-separated
    /// list, absent or empty meaning "any".
    ///
    /// # Errors
    ///
    /// Returns a message if `level` names something that is not a level. A
    /// typo there must not quietly become "no level filter" and return
    /// everything.
    pub fn from_params(
        text: &str,
        level: Option<&str>,
        app: Option<&str>,
        host: Option<&str>,
        stream_id: Option<&str>,
    ) -> Result<Self, String> {
        let names = |list: Option<&str>| -> Vec<NamePattern> {
            items(list).map(NamePattern::parse).collect()
        };
        Ok(Self {
            text: text.to_string(),
            levels: items(level).map(parse_level).collect::<Result<_, _>>()?,
            apps: names(app),
            hosts: names(host),
            streams: names(stream_id),
        })
    }

    #[must_use]
    pub fn matches(&self, r: &Record) -> bool {
        self.matches_columns(r.level, &r.app, &r.host, &r.stream_id, &r.body)
    }

    /// The same test on a row's column values, so a segment scan can reject
    /// a row before paying to decode its labels and fields.
    #[must_use]
    pub fn matches_columns(
        &self,
        level: Level,
        app: &str,
        host: &str,
        stream_id: &str,
        body: &str,
    ) -> bool {
        (self.levels.is_empty() || self.levels.contains(&level))
            && any_or_unset(&self.apps, app)
            && any_or_unset(&self.hosts, host)
            && any_or_unset(&self.streams, stream_id)
            && (self.text.is_empty() || body.contains(&self.text))
    }
}

fn items(list: Option<&str>) -> impl Iterator<Item = &str> {
    list.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn any_or_unset(patterns: &[NamePattern], value: &str) -> bool {
    patterns.is_empty() || patterns.iter().any(|p| p.matches(value))
}

fn parse_level(s: &str) -> Result<Level, String> {
    match s.to_ascii_lowercase().as_str() {
        "trace" => Ok(Level::Trace),
        "debug" => Ok(Level::Debug),
        "info" => Ok(Level::Info),
        "warn" | "warning" => Ok(Level::Warn),
        "error" | "err" => Ok(Level::Error),
        "fatal" => Ok(Level::Fatal),
        _ => Err(format!(
            "unknown level \"{s}\"; use trace, debug, info, warn, error or fatal"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;

    fn rec(app: &str, host: &str, stream: &str, level: Level, body: &str) -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 24, 12, 0, 0).unwrap(),
            host: host.into(),
            app: app.into(),
            stream_id: stream.into(),
            level,
            body: body.into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    #[test]
    fn the_default_filter_matches_everything() {
        let f = SearchFilter::default();
        assert!(f.matches(&rec("a", "h", "s", Level::Trace, "")));
    }

    #[test]
    fn every_set_constraint_must_hold() {
        let f = SearchFilter::from_params(
            "timeout",
            Some("error"),
            Some("billing"),
            Some("edge-1"),
            Some("billing-prod"),
        )
        .unwrap();
        let hit = rec(
            "billing",
            "edge-1",
            "billing-prod",
            Level::Error,
            "db timeout",
        );
        assert!(f.matches(&hit));
        assert!(!f.matches(&Record {
            level: Level::Warn,
            ..hit.clone()
        }));
        assert!(!f.matches(&Record {
            app: "web".into(),
            ..hit.clone()
        }));
        assert!(!f.matches(&Record {
            host: "edge-2".into(),
            ..hit.clone()
        }));
        assert!(!f.matches(&Record {
            stream_id: "billing-dev".into(),
            ..hit.clone()
        }));
        assert!(!f.matches(&Record {
            body: "db ok".into(),
            ..hit
        }));
    }

    #[test]
    fn alternatives_within_a_constraint_are_ored() {
        let f =
            SearchFilter::from_params("", Some("error, fatal"), Some("a,b"), None, None).unwrap();
        assert!(f.matches(&rec("a", "h", "s", Level::Fatal, "x")));
        assert!(f.matches(&rec("b", "h", "s", Level::Error, "x")));
        assert!(!f.matches(&rec("c", "h", "s", Level::Error, "x")));
        assert!(!f.matches(&rec("a", "h", "s", Level::Info, "x")));
    }

    #[test]
    fn a_trailing_star_matches_by_prefix() {
        let f = SearchFilter::from_params("", None, Some("pay*"), None, None).unwrap();
        assert!(f.matches(&rec("payments-api", "h", "s", Level::Info, "x")));
        assert!(f.matches(&rec("pay", "h", "s", Level::Info, "x")));
        assert!(!f.matches(&rec("web", "h", "s", Level::Info, "x")));
    }

    #[test]
    fn empty_and_absent_lists_mean_any() {
        let f = SearchFilter::from_params("", Some(""), Some(" , "), None, Some("")).unwrap();
        assert_eq!(f, SearchFilter::default());
    }

    #[test]
    fn level_names_are_case_insensitive_and_have_the_usual_aliases() {
        let f = SearchFilter::from_params("", Some("ERR,Warning"), None, None, None).unwrap();
        assert_eq!(f.levels, vec![Level::Error, Level::Warn]);
    }

    #[test]
    fn an_unknown_level_is_an_error_not_no_filter() {
        let e = SearchFilter::from_params("", Some("error,loud"), None, None, None).unwrap_err();
        assert!(e.contains("loud"), "{e}");
    }

    #[test]
    fn text_is_a_case_sensitive_substring() {
        let f = SearchFilter::text("Timeout");
        assert!(f.matches(&rec("a", "h", "s", Level::Info, "db Timeout x")));
        assert!(!f.matches(&rec("a", "h", "s", Level::Info, "db timeout x")));
    }
}
