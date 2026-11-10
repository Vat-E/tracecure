//! The query box's filter language.
//!
//! ```text
//! level:error,fatal app:billing-* host:edge-1 stream:api-prod connection timeout
//! ```
//!
//! `level:`, `app:`, `host:` and `stream:` are structured tokens; everything
//! else is free text, matched as one case-sensitive substring of the record
//! body — the same rule `/v1/search` applies to `q`, so a filter narrows the
//! loaded buffer exactly the way the server would have.
//!
//! Only those four keys are structured. A token like `postgres-lan:5432` has
//! a colon but is ordinary text, and a `level:` with a name that is not a
//! level is too: guessing at intent would silently drop what the user typed.

use tracecure_core::{Level, Record};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Free text. Empty = no text constraint. This is what goes to the
    /// server as `q`.
    pub text: String,
    /// Empty = any level.
    pub levels: Vec<Level>,
    /// Empty = any. An entry ending in `*` is a prefix match.
    pub apps: Vec<String>,
    pub hosts: Vec<String>,
    pub streams: Vec<String>,
}

impl Filter {
    #[must_use]
    pub fn parse(input: &str) -> Self {
        let mut f = Self::default();
        let mut free: Vec<&str> = Vec::new();
        for tok in input.split_whitespace() {
            match tok.split_once(':') {
                Some((key, value)) if !value.is_empty() => match key {
                    "level" | "lvl" => match parse_levels(value) {
                        Some(ls) => f.levels.extend(ls),
                        None => free.push(tok),
                    },
                    "app" => f.apps.extend(list(value)),
                    "host" => f.hosts.extend(list(value)),
                    "stream" => f.streams.extend(list(value)),
                    _ => free.push(tok),
                },
                _ => free.push(tok),
            }
        }
        f.text = free.join(" ");
        f
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
            && self.levels.is_empty()
            && self.apps.is_empty()
            && self.hosts.is_empty()
            && self.streams.is_empty()
    }

    #[must_use]
    pub fn matches(&self, r: &Record) -> bool {
        (self.levels.is_empty() || self.levels.contains(&r.level))
            && any_or_empty(&self.apps, &r.app)
            && any_or_empty(&self.hosts, &r.host)
            && any_or_empty(&self.streams, &r.stream_id)
            && (self.text.is_empty() || r.body.contains(&self.text))
    }
}

fn list(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_levels(value: &str) -> Option<Vec<Level>> {
    value
        .split(',')
        .filter(|s| !s.is_empty())
        .map(parse_level)
        .collect()
}

fn parse_level(s: &str) -> Option<Level> {
    match s.to_ascii_lowercase().as_str() {
        "trace" => Some(Level::Trace),
        "debug" => Some(Level::Debug),
        "info" => Some(Level::Info),
        "warn" | "warning" => Some(Level::Warn),
        "error" | "err" => Some(Level::Error),
        "fatal" => Some(Level::Fatal),
        _ => None,
    }
}

fn any_or_empty(patterns: &[String], value: &str) -> bool {
    patterns.is_empty() || patterns.iter().any(|p| glob(p, value))
}

fn glob(pattern: &str, value: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => value.starts_with(prefix),
        None => pattern == value,
    }
}

/// `query` with `key:value` added, unless it is already there.
///
/// Returns `None` when `value` cannot be written as a single token (it has
/// whitespace or a comma), rather than producing a query that parses to
/// something else.
#[must_use]
pub fn with_token(query: &str, key: &str, value: &str) -> Option<String> {
    if value.is_empty() || value.contains(|c: char| c.is_whitespace() || c == ',') {
        return None;
    }
    let token = format!("{key}:{value}");
    if query.split_whitespace().any(|t| t == token) {
        return Some(query.to_string());
    }
    let q = query.trim();
    Some(if q.is_empty() {
        token
    } else {
        format!("{token} {q}")
    })
}

/// Split `body` into runs, flagging the ones equal to `term`, for
/// highlighting. An empty `term` yields the whole body as one unflagged run.
#[must_use]
pub fn split_matches(body: &str, term: &str) -> Vec<(String, bool)> {
    if term.is_empty() {
        return vec![(body.to_string(), false)];
    }
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find(term) {
        if i > 0 {
            out.push((rest[..i].to_string(), false));
        }
        out.push((term.to_string(), true));
        rest = &rest[i + term.len()..];
    }
    if !rest.is_empty() || out.is_empty() {
        out.push((rest.to_string(), false));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;

    fn rec(app: &str, host: &str, lvl: Level, body: &str) -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 24, 12, 0, 0).unwrap(),
            host: host.into(),
            app: app.into(),
            stream_id: format!("{app}-prod"),
            level: lvl,
            body: body.into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    #[test]
    fn empty_input_is_an_empty_filter_that_matches_everything() {
        let f = Filter::parse("   ");
        assert!(f.is_empty());
        assert!(f.matches(&rec("a", "h", Level::Debug, "anything")));
    }

    #[test]
    fn structured_tokens_are_pulled_out_of_the_text() {
        let f = Filter::parse("level:error app:billing connection timeout host:edge-1");
        assert_eq!(f.levels, vec![Level::Error]);
        assert_eq!(f.apps, vec!["billing".to_string()]);
        assert_eq!(f.hosts, vec!["edge-1".to_string()]);
        assert_eq!(f.text, "connection timeout");
    }

    #[test]
    fn a_colon_in_ordinary_text_stays_text() {
        let f = Filter::parse("postgres-lan:5432 after");
        assert!(f.apps.is_empty() && f.levels.is_empty());
        assert_eq!(f.text, "postgres-lan:5432 after");
    }

    #[test]
    fn an_unknown_level_name_is_kept_as_text_not_dropped() {
        let f = Filter::parse("level:loud");
        assert!(f.levels.is_empty());
        assert_eq!(f.text, "level:loud");
    }

    #[test]
    fn comma_lists_and_aliases() {
        let f = Filter::parse("lvl:ERR,warning app:a,b");
        assert_eq!(f.levels, vec![Level::Error, Level::Warn]);
        assert_eq!(f.apps, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn matching_applies_every_constraint() {
        let f = Filter::parse("level:error app:billing timeout");
        assert!(f.matches(&rec("billing", "h", Level::Error, "db timeout")));
        assert!(!f.matches(&rec("billing", "h", Level::Warn, "db timeout")));
        assert!(!f.matches(&rec("front", "h", Level::Error, "db timeout")));
        assert!(!f.matches(&rec("billing", "h", Level::Error, "db ok")));
    }

    #[test]
    fn text_match_is_case_sensitive_like_the_server() {
        let f = Filter::parse("Timeout");
        assert!(!f.matches(&rec("a", "h", Level::Info, "db timeout")));
    }

    #[test]
    fn trailing_star_is_a_prefix_match() {
        let f = Filter::parse("app:robo*");
        assert!(f.matches(&rec("roboticket-api", "h", Level::Info, "x")));
        assert!(!f.matches(&rec("affine", "h", Level::Info, "x")));
    }

    #[test]
    fn stream_token_matches_stream_id() {
        let f = Filter::parse("stream:api-prod");
        assert!(f.matches(&rec("api", "h", Level::Info, "x")));
        assert!(!f.matches(&rec("web", "h", Level::Info, "x")));
    }

    #[test]
    fn with_token_prepends_once() {
        assert_eq!(with_token("", "app", "api").as_deref(), Some("app:api"));
        assert_eq!(
            with_token("timeout", "app", "api").as_deref(),
            Some("app:api timeout")
        );
        assert_eq!(
            with_token("app:api timeout", "app", "api").as_deref(),
            Some("app:api timeout")
        );
    }

    #[test]
    fn with_token_refuses_values_it_cannot_express() {
        assert_eq!(with_token("", "app", "my app"), None);
        assert_eq!(with_token("", "app", "a,b"), None);
        assert_eq!(with_token("", "app", ""), None);
    }

    #[test]
    fn split_matches_flags_every_occurrence() {
        assert_eq!(
            split_matches("a timeout, then timeout", "timeout"),
            vec![
                ("a ".to_string(), false),
                ("timeout".to_string(), true),
                (", then ".to_string(), false),
                ("timeout".to_string(), true),
            ]
        );
    }

    #[test]
    fn split_matches_without_a_term_or_a_hit_returns_the_body_whole() {
        assert_eq!(split_matches("abc", ""), vec![("abc".to_string(), false)]);
        assert_eq!(split_matches("abc", "zz"), vec![("abc".to_string(), false)]);
        assert_eq!(split_matches("", "zz"), vec![(String::new(), false)]);
    }
}
