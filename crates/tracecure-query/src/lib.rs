//! `TracecureQL` — the query language for Tracecure logs.
//!
//! ```text
//! {app="api", env="prod"} | level="error,fatal" | body ~ "timeout" | host!="edge-9"
//! ```
//!
//! A query is an optional selector in braces followed by any number of
//! `|`-separated stages. Both are lists of clauses, and a record matches when
//! **every** clause holds — the selector is not special, it is simply where
//! the clauses that pick a stream conventionally go.
//!
//! A clause is `key op "value"`:
//!
//! | `key` | means |
//! |---|---|
//! | `app`, `host`, `stream` (or `stream_id`), `level` (or `lvl`) | that column of the record |
//! | `body` | the message text |
//! | anything else | the label of that name |
//!
//! | `op` | means |
//! |---|---|
//! | `=` | equal to one of the comma-separated alternatives; an alternative ending in `*` is a prefix |
//! | `!=` | equal to none of them |
//! | `~` | contains the value as a case-sensitive substring |
//! | `!~` | does not contain it |
//!
//! A record that has no such label fails `=` and `~`, and passes `!=` and
//! `!~`: "is not prod" is true of a record that says nothing about `env`.
//!
//! This crate only parses and matches. It does not read storage; the storage
//! layer evaluates the same [`Clause`]s against segment columns, and the
//! browser UI evaluates them against live records, so all three agree by
//! construction.

use std::fmt;

use tracecure_core::{Level, Record};

/// A parsed query: every clause must hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub clauses: Vec<Clause>,
}

/// One `key op "value"` test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    pub field: Field,
    pub test: Test,
}

/// What a clause looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    App,
    Host,
    Stream,
    Level,
    Body,
    Label(String),
}

/// What a clause requires of the field's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Test {
    /// `=`: one of these. An entry ending in `*` matches by prefix.
    AnyOf(Vec<String>),
    /// `!=`: none of these.
    NoneOf(Vec<String>),
    /// `~`: contains this substring.
    Contains(String),
    /// `!~`: does not contain it.
    NotContains(String),
}

/// Why a query could not be parsed, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset into the input.
    pub at: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at position {})", self.message, self.at)
    }
}

impl std::error::Error for ParseError {}

/// The wire name of a level.
#[must_use]
pub fn level_name(level: Level) -> &'static str {
    match level {
        Level::Trace => "trace",
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
        Level::Fatal => "fatal",
    }
}

fn canonical_level(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "trace" => "trace",
        "debug" => "debug",
        "info" => "info",
        "warn" | "warning" => "warn",
        "error" | "err" => "error",
        "fatal" => "fatal",
        _ => return None,
    })
}

/// Whether `input` is meant as `TracecureQL` rather than as the plain filter
/// syntax of the query box. A query always starts with `{` or `|`; plain text
/// that happens to start that way has to be quoted some other way.
#[must_use]
pub fn looks_like_query(input: &str) -> bool {
    matches!(input.trim_start().as_bytes().first(), Some(b'{' | b'|'))
}

impl Test {
    /// Apply the test to a field's value; `None` is a label the record does
    /// not have.
    #[must_use]
    pub fn accepts(&self, value: Option<&str>) -> bool {
        match (self, value) {
            (Self::AnyOf(alts), Some(v)) => alts.iter().any(|a| alternative_matches(a, v)),
            (Self::NoneOf(alts), Some(v)) => !alts.iter().any(|a| alternative_matches(a, v)),
            (Self::Contains(needle), Some(v)) => v.contains(needle.as_str()),
            (Self::NotContains(needle), Some(v)) => !v.contains(needle.as_str()),
            (Self::AnyOf(_) | Self::Contains(_), None) => false,
            (Self::NoneOf(_) | Self::NotContains(_), None) => true,
        }
    }
}

fn alternative_matches(alternative: &str, value: &str) -> bool {
    match alternative.strip_suffix('*') {
        Some(prefix) => value.starts_with(prefix),
        None => alternative == value,
    }
}

impl Clause {
    #[must_use]
    pub fn matches(&self, r: &Record) -> bool {
        let value = match &self.field {
            Field::App => Some(r.app.as_str()),
            Field::Host => Some(r.host.as_str()),
            Field::Stream => Some(r.stream_id.as_str()),
            Field::Level => Some(level_name(r.level)),
            Field::Body => Some(r.body.as_str()),
            Field::Label(key) => r.labels.get(key).map(String::as_str),
        };
        self.test.accepts(value)
    }
}

impl Query {
    /// Parse a query.
    ///
    /// # Errors
    ///
    /// Returns where and why the input is not a query. An unknown level name
    /// is an error rather than a clause that matches nothing: a typo must not
    /// look like "no errors today".
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        Parser::new(input).query()
    }

    #[must_use]
    pub fn matches(&self, r: &Record) -> bool {
        self.clauses.iter().all(|c| c.matches(r))
    }

    /// Whether any clause looks at a label — the expensive column to read.
    #[must_use]
    pub fn uses_labels(&self) -> bool {
        self.clauses
            .iter()
            .any(|c| matches!(c.field, Field::Label(_)))
    }
}

fn fail<T>(at: usize, message: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError {
        at,
        message: message.into(),
    })
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn skip_space(&mut self) {
        self.pos += self.rest().len() - self.rest().trim_start().len();
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip_space();
        if self.rest().starts_with(token) {
            self.pos += token.len();
            true
        } else {
            false
        }
    }

    fn at_end(&mut self) -> bool {
        self.skip_space();
        self.rest().is_empty()
    }

    fn query(mut self) -> Result<Query, ParseError> {
        let mut clauses = Vec::new();
        if self.at_end() {
            return fail(0, "empty query");
        }

        if self.eat("{") {
            if !self.eat("}") {
                loop {
                    clauses.push(self.clause()?);
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("}") {
                        break;
                    }
                    return fail(self.pos, "expected `,` or `}` in the selector");
                }
            }
        } else if !self.rest().starts_with('|') {
            return fail(self.pos, "a query starts with `{` or `|`");
        }

        while !self.at_end() {
            if !self.eat("|") {
                return fail(self.pos, "expected `|` before the next stage");
            }
            clauses.push(self.clause()?);
        }
        Ok(Query { clauses })
    }

    fn clause(&mut self) -> Result<Clause, ParseError> {
        self.skip_space();
        let key_at = self.pos;
        let key = self.key()?;
        let op_at = {
            self.skip_space();
            self.pos
        };
        // Longest first: `!=` and `!~` before `=` and `~`.
        let op = ["!=", "!~", "=", "~"]
            .into_iter()
            .find(|op| self.eat(op))
            .ok_or(ParseError {
                at: op_at,
                message: format!("expected `=`, `!=`, `~` or `!~` after `{key}`"),
            })?;
        self.skip_space();
        let value_at = self.pos;
        let value = self.string()?;

        let field = match key {
            "app" => Field::App,
            "host" => Field::Host,
            "stream" | "stream_id" => Field::Stream,
            "level" | "lvl" => Field::Level,
            "body" => Field::Body,
            other => Field::Label(other.to_string()),
        };

        let test = match op {
            "~" => Test::Contains(value),
            "!~" => Test::NotContains(value),
            _ => {
                let mut alternatives: Vec<String> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                if alternatives.is_empty() {
                    // `env=""` is a real question: "the label is empty".
                    alternatives.push(String::new());
                }
                if field == Field::Level {
                    for alt in &mut alternatives {
                        match canonical_level(alt) {
                            Some(name) => *alt = name.to_string(),
                            None => {
                                return fail(
                                    value_at,
                                    format!(
                                        "unknown level \"{alt}\"; use trace, debug, info, warn, error or fatal"
                                    ),
                                )
                            }
                        }
                    }
                }
                if op == "=" {
                    Test::AnyOf(alternatives)
                } else {
                    Test::NoneOf(alternatives)
                }
            }
        };

        if field == Field::Level && matches!(test, Test::Contains(_) | Test::NotContains(_)) {
            return fail(key_at, "`level` takes `=` or `!=`, not `~`");
        }
        Ok(Clause { field, test })
    }

    fn key(&mut self) -> Result<&'a str, ParseError> {
        let rest = self.rest();
        let len = rest
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-' | '/')))
            .unwrap_or(rest.len());
        if len == 0 {
            return fail(self.pos, "expected a field or label name");
        }
        self.pos += len;
        Ok(&rest[..len])
    }

    fn string(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        let mut chars = self.rest().char_indices();
        if !matches!(chars.next(), Some((_, '"'))) {
            return fail(start, "expected a value in double quotes");
        }
        let mut out = String::new();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => {
                    self.pos = start + i + 1;
                    return Ok(out);
                }
                '\\' => match chars.next() {
                    Some((_, e @ ('"' | '\\'))) => out.push(e),
                    Some((_, 'n')) => out.push('\n'),
                    Some((_, 't')) => out.push('\t'),
                    Some((j, other)) => {
                        return fail(start + j, format!("unknown escape `\\{other}`"))
                    }
                    None => break,
                },
                other => out.push(other),
            }
        }
        fail(start, "the value's closing quote is missing")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn rec() -> Record {
        Record {
            timestamp: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            host: "edge-1".into(),
            app: "billing-api".into(),
            stream_id: "api-prod".into(),
            level: Level::Error,
            body: "connection timeout after 30s".into(),
            labels: BTreeMap::from([
                ("env".to_string(), "prod".to_string()),
                ("region".to_string(), "eu-west".to_string()),
            ]),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    fn q(s: &str) -> Query {
        Query::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    fn any(values: &[&str]) -> Test {
        Test::AnyOf(values.iter().map(ToString::to_string).collect())
    }

    #[test]
    fn parses_the_example_from_the_plan() {
        let query = q(r#"{app="api", env="prod"} | level="error" | body ~ "timeout""#);
        assert_eq!(
            query.clauses,
            vec![
                Clause {
                    field: Field::App,
                    test: any(&["api"])
                },
                Clause {
                    field: Field::Label("env".into()),
                    test: any(&["prod"])
                },
                Clause {
                    field: Field::Level,
                    test: any(&["error"])
                },
                Clause {
                    field: Field::Body,
                    test: Test::Contains("timeout".into())
                },
            ]
        );
    }

    #[test]
    fn selector_and_stages_are_each_optional() {
        assert!(q("{}").clauses.is_empty());
        assert_eq!(q(r#"{app="a"}"#).clauses.len(), 1);
        assert_eq!(q(r#"| level="warn""#).clauses.len(), 1);
        assert_eq!(q(r#"{} | body ~ "x" | body !~ "y""#).clauses.len(), 2);
    }

    #[test]
    fn whitespace_is_free_and_aliases_work() {
        let a = q(r#"{stream_id="s",lvl="ERR"}|body~"x""#);
        let b = q(r#"  {  stream = "s" ,  level = "error"  }  |  body  ~  "x"  "#);
        assert_eq!(a, b);
    }

    #[test]
    fn strings_keep_their_contents_and_unescape() {
        let query = q(r#"| body ~ "a | b, {c} \"quoted\" \\ back""#);
        assert_eq!(
            query.clauses[0].test,
            Test::Contains(r#"a | b, {c} "quoted" \ back"#.into())
        );
    }

    #[test]
    fn matches_every_kind_of_clause() {
        let r = rec();
        for yes in [
            "{}",
            r#"{app="billing-api"}"#,
            r#"{app="billing-*"}"#,
            r#"{app="worker,billing-*"}"#,
            r#"{host!="edge-2", stream="api-prod"}"#,
            r#"{env="prod", region="eu-*"}"#,
            r#"| level="error,fatal""#,
            r#"| level!="debug,trace""#,
            r#"| body ~ "timeout" | body !~ "refused""#,
            r#"| env ~ "ro""#,
            // A label the record does not have: "is not" holds, "is" does not.
            r#"| tenant!="acme""#,
            r#"| tenant !~ "acme""#,
        ] {
            assert!(q(yes).matches(&r), "should match: {yes}");
        }
        for no in [
            r#"{app="billing"}"#,
            r#"{app!="billing-*"}"#,
            r#"{env="staging"}"#,
            r#"| level="info""#,
            r#"| level!="error""#,
            r#"| body ~ "Timeout""#,
            r#"| body !~ "timeout""#,
            r#"| tenant="acme""#,
            r#"| tenant ~ "a""#,
            r#"{app="billing-api"} | env="dev""#,
        ] {
            assert!(!q(no).matches(&r), "should not match: {no}");
        }
    }

    #[test]
    fn an_empty_value_asks_for_an_empty_label() {
        let mut r = rec();
        assert!(!q(r#"{env=""}"#).matches(&r));
        r.labels.insert("env".into(), String::new());
        assert!(q(r#"{env=""}"#).matches(&r));
    }

    #[test]
    fn errors_say_what_and_where() {
        let cases = [
            ("", 0, "empty query"),
            ("app=\"x\"", 0, "starts with"),
            ("{app}", 4, "expected `=`"),
            ("{app=x}", 5, "double quotes"),
            ("{app=\"x\"", 8, "expected `,` or `}`"),
            ("{app=\"x\"} level=\"error\"", 10, "expected `|`"),
            ("{app=\"x", 5, "closing quote"),
            ("| level=\"eror\"", 8, "unknown level"),
            ("| level ~ \"err\"", 2, "`level` takes"),
            ("| =\"x\"", 2, "expected a field"),
            ("| body ~ \"a\\qb\"", 12, "unknown escape"),
        ];
        for (input, at, needle) in cases {
            let e = Query::parse(input).expect_err(input);
            assert!(e.message.contains(needle), "{input}: {e}");
            assert_eq!(e.at, at, "{input}: {e}");
        }
    }

    #[test]
    fn tells_a_query_from_plain_filter_text() {
        assert!(looks_like_query(r#"{app="x"}"#));
        assert!(looks_like_query(r#"  | body ~ "x""#));
        assert!(!looks_like_query("level:error timeout"));
        assert!(!looks_like_query(""));
    }

    #[test]
    fn knows_when_it_needs_labels() {
        assert!(q(r#"{env="prod"}"#).uses_labels());
        assert!(!q(r#"{app="a"} | body ~ "x""#).uses_labels());
    }
}
