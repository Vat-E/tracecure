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
//! Input that starts with `{` or `|` is not this syntax at all: it is a
//! `TracecureQL` query (see the `tracecure-query` crate), which can also
//! negate and ask about labels. It is kept whole in [`Filter::ql`].
//!
//! Only those four keys are structured. A token like `postgres-lan:5432` has
//! a colon but is ordinary text, and a `level:` with a name that is not a
//! level is too: guessing at intent would silently drop what the user typed.

use tracecure_core::{Level, Record};
use tracecure_query::Query;

/// A `TracecureQL` query as typed, and what it parsed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ql {
    pub source: String,
    /// The parse error, worded for the user, when it is not a query yet.
    pub parsed: Result<Query, String>,
}

impl Ql {
    /// While a query is still being typed it does not parse; hiding every
    /// record until the closing quote arrives would be worse than showing
    /// them, so an unparsed query filters nothing.
    fn matches(&self, r: &Record) -> bool {
        self.parsed.as_ref().map_or(true, |q| q.matches(r))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Set when the input is `TracecureQL`; the other fields are then empty.
    pub ql: Option<Ql>,
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
        if tracecure_query::looks_like_query(input) {
            return Self {
                ql: Some(Ql {
                    source: input.trim().to_string(),
                    parsed: Query::parse(input.trim()).map_err(|e| e.to_string()),
                }),
                ..Self::default()
            };
        }
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
        self.ql.is_none()
            && self.text.is_empty()
            && self.levels.is_empty()
            && self.apps.is_empty()
            && self.hosts.is_empty()
            && self.streams.is_empty()
    }

    #[must_use]
    pub fn matches(&self, r: &Record) -> bool {
        self.ql.as_ref().is_none_or(|ql| ql.matches(r))
            && (self.levels.is_empty() || self.levels.contains(&r.level))
            && any_or_empty(&self.apps, &r.app)
            && any_or_empty(&self.hosts, &r.host)
            && any_or_empty(&self.streams, &r.stream_id)
            && (self.text.is_empty() || r.body.contains(&self.text))
    }
}

/// Every level, least to most severe. The order `ServerFilter` lists levels
/// in, so two filters meaning the same thing compare equal.
pub const ALL_LEVELS: [Level; 6] = [
    Level::Trace,
    Level::Debug,
    Level::Info,
    Level::Warn,
    Level::Error,
    Level::Fatal,
];

/// What a fetch asks the server for — and therefore what the buffer holds.
///
/// Built from two things the user controls separately: the query box
/// (`Filter`) and the level buttons. Sending both is what makes the buffer
/// trustworthy: the server applies them *before* its `limit`, so "the newest
/// 2000 errors" is what comes back, rather than whichever errors happen to
/// be among the newest 2000 records of any kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerFilter {
    pub text: String,
    /// `None` = any level. `Some(vec![])` = no level at all, which nothing
    /// can match (every level button off, or a `level:` token naming only
    /// levels whose buttons are off).
    pub levels: Option<Vec<Level>>,
    pub apps: Vec<String>,
    pub hosts: Vec<String>,
    pub streams: Vec<String>,
    /// A `TracecureQL` query, sent to `/v1/query` instead of `/v1/search`.
    /// The level buttons still apply, as one more stage.
    pub ql: Option<Ql>,
}

impl ServerFilter {
    /// Combine the query box with the levels whose buttons are on.
    ///
    /// A `level:` token narrows within the enabled levels; it does not
    /// override a button that is off.
    #[must_use]
    pub fn new(filter: &Filter, enabled: &[Level]) -> Self {
        let wanted = |l: &Level| filter.levels.is_empty() || filter.levels.contains(l);
        let levels: Vec<Level> = ALL_LEVELS
            .into_iter()
            .filter(|l| enabled.contains(l) && wanted(l))
            .collect();
        Self {
            text: filter.text.clone(),
            levels: (levels.len() < ALL_LEVELS.len()).then_some(levels),
            apps: filter.apps.clone(),
            hosts: filter.hosts.clone(),
            streams: filter.streams.clone(),
            ql: filter.ql.clone(),
        }
    }

    /// The query to send to `/v1/query`: what was typed, plus the level
    /// buttons as a final stage. `None` when the query box holds the plain
    /// filter syntax; `Some(Err(_))` when it holds a query that does not
    /// parse, with the reason.
    #[must_use]
    pub fn query_text(&self) -> Option<Result<String, String>> {
        let ql = self.ql.as_ref()?;
        Some(match &ql.parsed {
            Err(e) => Err(e.clone()),
            Ok(_) => Ok(match self.level_param() {
                Some(levels) => format!("{} | level=\"{levels}\"", ql.source),
                None => ql.source.clone(),
            }),
        })
    }

    /// True when no record can match, so there is nothing to ask the server.
    #[must_use]
    pub fn matches_nothing(&self) -> bool {
        self.levels.as_ref().is_some_and(Vec::is_empty)
    }

    /// Would the server have returned this record for this filter? Used to
    /// keep live records that fall outside a loaded search out of the buffer.
    #[must_use]
    pub fn matches(&self, r: &Record) -> bool {
        self.levels.as_ref().is_none_or(|ls| ls.contains(&r.level))
            && self.ql.as_ref().is_none_or(|ql| ql.matches(r))
            && any_or_empty(&self.apps, &r.app)
            && any_or_empty(&self.hosts, &r.host)
            && any_or_empty(&self.streams, &r.stream_id)
            && (self.text.is_empty() || r.body.contains(&self.text))
    }

    /// The query-box part of the filter, written the way it is typed —
    /// `app:billing timeout` — for saying what the buffer was loaded for.
    /// Levels are left out: the buttons already show them.
    #[must_use]
    pub fn describe(&self) -> String {
        if let Some(ql) = &self.ql {
            return ql.source.clone();
        }
        let mut parts: Vec<String> = Vec::new();
        for (key, values) in [
            ("app", &self.apps),
            ("host", &self.hosts),
            ("stream", &self.streams),
        ] {
            if !values.is_empty() {
                parts.push(format!("{key}:{}", values.join(",")));
            }
        }
        if !self.text.is_empty() {
            parts.push(self.text.clone());
        }
        parts.join(" ")
    }

    /// The level names for the wire, comma-separated. `None` when any level
    /// is acceptable and the parameter should be left out.
    #[must_use]
    pub fn level_param(&self) -> Option<String> {
        self.levels.as_ref().map(|ls| {
            ls.iter()
                .map(|l| level_name(*l))
                .collect::<Vec<_>>()
                .join(",")
        })
    }
}

/// A filter on body text alone — what every fetch was before the server
/// learned the other constraints.
impl From<&str> for ServerFilter {
    fn from(text: &str) -> Self {
        Self {
            text: text.to_string(),
            ..Self::default()
        }
    }
}

impl From<String> for ServerFilter {
    fn from(text: String) -> Self {
        Self {
            text,
            ..Self::default()
        }
    }
}

#[must_use]
pub fn level_name(l: Level) -> &'static str {
    match l {
        Level::Trace => "trace",
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
        Level::Fatal => "fatal",
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
    if tracecure_query::looks_like_query(query) {
        // The box holds a query: add a stage to it rather than a token in
        // a syntax it is not written in.
        if value.contains(['"', '\\']) {
            return None;
        }
        let stage = format!("{key}=\"{value}\"");
        let q = query.trim();
        return Some(if q.contains(&stage) {
            q.to_string()
        } else {
            format!("{q} | {stage}")
        });
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
        let f = Filter::parse("app:pay*");
        assert!(f.matches(&rec("payments-api", "h", Level::Info, "x")));
        assert!(!f.matches(&rec("web", "h", Level::Info, "x")));
    }

    #[test]
    fn stream_token_matches_stream_id() {
        let f = Filter::parse("stream:api-prod");
        assert!(f.matches(&rec("api", "h", Level::Info, "x")));
        assert!(!f.matches(&rec("web", "h", Level::Info, "x")));
    }

    const DEFAULT_ON: [Level; 4] = [Level::Info, Level::Warn, Level::Error, Level::Fatal];

    #[test]
    fn input_starting_with_a_brace_or_pipe_is_a_query() {
        let f = Filter::parse(r#" {app="pay*", env="prod"} | body ~ "timeout" "#);
        let ql = f.ql.as_ref().expect("a query");
        assert_eq!(ql.source, r#"{app="pay*", env="prod"} | body ~ "timeout""#);
        assert!(ql.parsed.is_ok());
        assert!(f.text.is_empty() && f.apps.is_empty() && !f.is_empty());

        let mut r = rec("payments-api", "edge-1", Level::Error, "connection timeout");
        assert!(!f.matches(&r), "no env label yet");
        r.labels.insert("env".into(), "prod".into());
        assert!(f.matches(&r));
        r.body = "connection refused".into();
        assert!(!f.matches(&r));
    }

    #[test]
    fn a_query_that_does_not_parse_yet_hides_nothing_and_says_why() {
        let f = Filter::parse(r#"{app="pay"#);
        let ql = f.ql.as_ref().expect("a query");
        assert!(ql.parsed.as_ref().unwrap_err().contains("closing quote"));
        assert!(f.matches(&rec("web", "edge-1", Level::Info, "anything")));

        let s = ServerFilter::new(&f, &ALL_LEVELS);
        assert!(s.query_text().unwrap().is_err());
    }

    #[test]
    fn the_level_buttons_become_a_stage_of_the_query() {
        let f = Filter::parse(r#"{app="web"}"#);
        let all = ServerFilter::new(&f, &ALL_LEVELS);
        assert_eq!(all.query_text(), Some(Ok(r#"{app="web"}"#.to_string())));

        let some = ServerFilter::new(&f, &[Level::Warn, Level::Error]);
        assert_eq!(
            some.query_text(),
            Some(Ok(r#"{app="web"} | level="warn,error""#.to_string()))
        );
        assert!(some.matches(&rec("web", "h", Level::Error, "x")));
        assert!(!some.matches(&rec("web", "h", Level::Info, "x")));
        assert!(!some.matches(&rec("api", "h", Level::Error, "x")));
        assert_eq!(some.describe(), r#"{app="web"}"#);

        // The plain syntax is not a query.
        let plain = ServerFilter::new(&Filter::parse("app:web"), &ALL_LEVELS);
        assert_eq!(plain.query_text(), None);
    }

    #[test]
    fn adding_a_token_to_a_query_adds_a_stage() {
        assert_eq!(
            with_token(r#"{app="web"}"#, "host", "edge-1").as_deref(),
            Some(r#"{app="web"} | host="edge-1""#)
        );
        assert_eq!(
            with_token(r#"{app="web"} | host="edge-1""#, "host", "edge-1").as_deref(),
            Some(r#"{app="web"} | host="edge-1""#)
        );
        assert_eq!(with_token("{}", "app", r#"a"b"#), None);
    }

    #[test]
    fn server_filter_with_every_level_on_sends_no_level_constraint() {
        let f = ServerFilter::new(&Filter::parse("timeout"), &ALL_LEVELS);
        assert_eq!(f.levels, None);
        assert_eq!(f.level_param(), None);
        assert_eq!(f.text, "timeout");
    }

    #[test]
    fn server_filter_sends_the_enabled_levels_in_severity_order() {
        let f = ServerFilter::new(
            &Filter::default(),
            &[Level::Fatal, Level::Info, Level::Error],
        );
        assert_eq!(f.level_param().as_deref(), Some("info,error,fatal"));
    }

    #[test]
    fn a_level_token_narrows_within_the_enabled_levels() {
        let f = ServerFilter::new(&Filter::parse("level:error,debug"), &DEFAULT_ON);
        // `debug` is asked for but its button is off: it stays off.
        assert_eq!(f.levels, Some(vec![Level::Error]));
    }

    #[test]
    fn a_level_token_for_a_disabled_level_matches_nothing() {
        let f = ServerFilter::new(&Filter::parse("level:debug"), &DEFAULT_ON);
        assert!(f.matches_nothing());
        assert!(!f.matches(&rec("a", "h", Level::Debug, "x")));
    }

    #[test]
    fn every_button_off_matches_nothing() {
        let f = ServerFilter::new(&Filter::default(), &[]);
        assert!(f.matches_nothing());
        assert_eq!(f.level_param().as_deref(), Some(""));
    }

    #[test]
    fn server_filter_matches_what_the_server_would_return() {
        let f = ServerFilter::new(&Filter::parse("app:pay* timeout"), &DEFAULT_ON);
        assert!(f.matches(&rec("payments-api", "h", Level::Error, "db timeout")));
        assert!(!f.matches(&rec("payments-api", "h", Level::Debug, "db timeout")));
        assert!(!f.matches(&rec("web", "h", Level::Error, "db timeout")));
        assert!(!f.matches(&rec("payments-api", "h", Level::Error, "db ok")));
    }

    #[test]
    fn the_same_request_built_two_ways_compares_equal() {
        let a = ServerFilter::new(&Filter::parse("level:fatal,error x"), &DEFAULT_ON);
        let b = ServerFilter::new(&Filter::parse("x level:error,fatal"), &DEFAULT_ON);
        assert_eq!(a, b);
    }

    #[test]
    fn describe_writes_the_query_part_the_way_it_is_typed() {
        let f = ServerFilter::new(
            &Filter::parse("timeout app:billing,web host:edge-1 level:error"),
            &DEFAULT_ON,
        );
        assert_eq!(f.describe(), "app:billing,web host:edge-1 timeout");
        assert_eq!(ServerFilter::default().describe(), "");
    }

    #[test]
    fn a_plain_string_is_a_text_only_filter() {
        let f: ServerFilter = "boom".into();
        assert_eq!(f.text, "boom");
        assert_eq!(f.levels, None);
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
