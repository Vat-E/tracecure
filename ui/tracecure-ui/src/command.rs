//! `:` command palette — parser + completion.
//!
//! Grammar (EBNF-ish):
//!   command  ::= ':' verb (SP kv)*
//!   verb     ::= 'pause' | 'resume' | 'clear' | 'retry' | 'save'
//!              | 'search' | 'level' | 'help' | 'quit'
//!   kv       ::= key '=' value        | `bare_arg` (for `:level`)
//!   key      ::= [a-z][a-z0-9_]*
//!   value    ::= '"' ... '"' | [^ "=]+

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use sift_core::Level;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAction {
    Pause,
    Resume,
    Clear,
    Retry,
    Save {
        path: PathBuf,
    },
    Search {
        q: Option<String>,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    },
    Level {
        level: Level,
        on: bool,
    },
    Help,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    Empty,
    UnknownVerb(String),
    UnknownKey {
        verb: &'static str,
        key: String,
    },
    MissingKey {
        verb: &'static str,
        key: &'static str,
    },
    BadValue {
        key: &'static str,
        reason: String,
    },
    BadTime {
        key: &'static str,
        value: String,
    },
    BadLevel(String),
    BadToggle(String),
    UnterminatedQuote(String),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty command"),
            Self::UnknownVerb(v) => write!(f, "unknown verb \"{v}\""),
            Self::UnknownKey { verb, key } => write!(f, ":{verb} takes no `{key}` key"),
            Self::MissingKey { verb, key } => write!(f, ":{verb} requires `{key}`"),
            Self::BadValue { key, reason } => write!(f, "{key}: {reason}"),
            Self::BadTime { key, value } => write!(f, "{key}=\"{value}\" is not RFC3339"),
            Self::BadLevel(s) => write!(f, "unknown level \"{s}\""),
            Self::BadToggle(s) => write!(f, "expected `on` or `off`, got \"{s}\""),
            Self::UnterminatedQuote(s) => write!(f, "unterminated quote in \"{s}\""),
        }
    }
}

impl std::error::Error for CommandError {}

const ALL_VERBS: &[&str] = &[
    "pause", "resume", "clear", "retry", "save", "search", "level", "help", "quit",
];

/// Parse a user-typed command string. Input may or may not start with `:` — we
/// accept both.
///
/// # Errors
///
/// Returns an error on any grammar or type problem.
pub fn parse(input: &str) -> Result<CommandAction, CommandError> {
    let input = input.trim_start_matches(':').trim();
    if input.is_empty() {
        return Err(CommandError::Empty);
    }

    let tokens = tokenize(input)?;
    let mut iter = tokens.iter().map(String::as_str);
    let verb = iter.next().ok_or(CommandError::Empty)?;
    let rest: Vec<&str> = iter.collect();

    match verb {
        "pause" => Ok(CommandAction::Pause),
        "resume" => Ok(CommandAction::Resume),
        "clear" => Ok(CommandAction::Clear),
        "retry" => Ok(CommandAction::Retry),
        "help" => Ok(CommandAction::Help),
        "quit" => Ok(CommandAction::Quit),
        "save" => parse_save(&rest),
        "search" => parse_search(&rest),
        "level" => parse_level(&rest),
        other => Err(CommandError::UnknownVerb(other.into())),
    }
}

/// Split `input` into whitespace-delimited tokens, but keep the contents of
/// double-quoted regions intact (the quote characters themselves are preserved
/// so that `unquote` can strip them later). A quote that opens inside a
/// non-whitespace run keeps that run together as one token.
fn tokenize(input: &str) -> Result<Vec<String>, CommandError> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in input.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    if in_quotes {
        return Err(CommandError::UnterminatedQuote(input.to_string()));
    }
    Ok(tokens)
}

fn parse_save(args: &[&str]) -> Result<CommandAction, CommandError> {
    let mut path: Option<PathBuf> = None;
    for a in args {
        match split_kv(a) {
            Some(("path", v)) => path = Some(PathBuf::from(v)),
            Some((k, _)) => {
                return Err(CommandError::UnknownKey {
                    verb: "save",
                    key: k.into(),
                })
            }
            None => {
                return Err(CommandError::BadValue {
                    key: "save",
                    reason: format!("expected key=value, got \"{a}\""),
                })
            }
        }
    }
    let path = path.ok_or(CommandError::MissingKey {
        verb: "save",
        key: "path",
    })?;
    Ok(CommandAction::Save { path })
}

fn parse_search(args: &[&str]) -> Result<CommandAction, CommandError> {
    let mut q: Option<String> = None;
    let mut from: Option<DateTime<Utc>> = None;
    let mut to: Option<DateTime<Utc>> = None;
    for a in args {
        match split_kv(a) {
            Some(("q", v)) => q = Some(unquote(v).to_string()),
            Some(("from", v)) => {
                from = Some(parse_rfc3339(v, "from")?);
            }
            Some(("to", v)) => {
                to = Some(parse_rfc3339(v, "to")?);
            }
            Some((k, _)) => {
                return Err(CommandError::UnknownKey {
                    verb: "search",
                    key: k.into(),
                })
            }
            None => {
                return Err(CommandError::BadValue {
                    key: "search",
                    reason: format!("expected key=value, got \"{a}\""),
                })
            }
        }
    }
    Ok(CommandAction::Search { q, from, to })
}

fn parse_level(args: &[&str]) -> Result<CommandAction, CommandError> {
    if args.len() != 2 {
        return Err(CommandError::BadValue {
            key: "level",
            reason: "usage: :level <name> on|off".into(),
        });
    }
    let level = parse_level_name(args[0])?;
    let on = match args[1] {
        "on" | "true" => true,
        "off" | "false" => false,
        other => return Err(CommandError::BadToggle(other.into())),
    };
    Ok(CommandAction::Level { level, on })
}

fn parse_level_name(s: &str) -> Result<Level, CommandError> {
    match s.to_ascii_lowercase().as_str() {
        "trace" => Ok(Level::Trace),
        "debug" => Ok(Level::Debug),
        "info" => Ok(Level::Info),
        "warn" => Ok(Level::Warn),
        "error" => Ok(Level::Error),
        "fatal" => Ok(Level::Fatal),
        _ => Err(CommandError::BadLevel(s.into())),
    }
}

fn parse_rfc3339(v: &str, key: &'static str) -> Result<DateTime<Utc>, CommandError> {
    DateTime::parse_from_rfc3339(unquote(v))
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| CommandError::BadTime {
            key,
            value: v.into(),
        })
}

fn split_kv(s: &str) -> Option<(&str, &str)> {
    s.split_once('=')
}

fn unquote(s: &str) -> &str {
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Tab-completion result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Longest shared prefix of all matching verbs (may equal the input itself
    /// when no unique completion exists).
    pub prefix: String,
    /// All matching verbs (for display — unused by the MVP UI).
    pub matches: Vec<&'static str>,
}

/// Tab-complete the verb portion of a command. `input` may or may not start
/// with `:`. Only completes the first word; does nothing useful if the input
/// already has whitespace.
#[must_use]
pub fn complete(input: &str) -> Completion {
    let stripped = input.trim_start_matches(':');
    let partial = stripped.split_whitespace().next().unwrap_or("");
    let matches: Vec<&'static str> = ALL_VERBS
        .iter()
        .copied()
        .filter(|v| v.starts_with(partial))
        .collect();
    let prefix = longest_shared_prefix(&matches).to_string();
    Completion { prefix, matches }
}

fn longest_shared_prefix(words: &[&'static str]) -> &'static str {
    match words {
        [] => "",
        [only] => only,
        [first, rest @ ..] => {
            let mut upper = first.len();
            for w in rest {
                let mut common = 0;
                for (a, b) in first.as_bytes().iter().zip(w.as_bytes().iter()) {
                    if a == b {
                        common += 1;
                    } else {
                        break;
                    }
                }
                upper = upper.min(common);
            }
            &first[..upper]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bare_verbs_returns_expected_action() {
        assert_eq!(parse(":pause").unwrap(), CommandAction::Pause);
        assert_eq!(parse(":resume").unwrap(), CommandAction::Resume);
        assert_eq!(parse(":clear").unwrap(), CommandAction::Clear);
        assert_eq!(parse(":retry").unwrap(), CommandAction::Retry);
        assert_eq!(parse(":help").unwrap(), CommandAction::Help);
        assert_eq!(parse(":quit").unwrap(), CommandAction::Quit);
    }

    #[test]
    fn colon_prefix_is_optional() {
        assert_eq!(parse("pause").unwrap(), CommandAction::Pause);
    }

    #[test]
    fn empty_is_err_empty() {
        assert_eq!(parse("").unwrap_err(), CommandError::Empty);
        assert_eq!(parse(":").unwrap_err(), CommandError::Empty);
        assert_eq!(parse("   ").unwrap_err(), CommandError::Empty);
    }

    #[test]
    fn unknown_verb_reports_name() {
        match parse(":frobnicate").unwrap_err() {
            CommandError::UnknownVerb(v) => assert_eq!(v, "frobnicate"),
            other => panic!("expected UnknownVerb, got {other:?}"),
        }
    }

    #[test]
    fn save_requires_path() {
        assert!(matches!(
            parse(":save").unwrap_err(),
            CommandError::MissingKey {
                verb: "save",
                key: "path"
            }
        ));
    }

    #[test]
    fn save_accepts_path() {
        let got = parse(":save path=foo.ndjson").unwrap();
        assert_eq!(
            got,
            CommandAction::Save {
                path: PathBuf::from("foo.ndjson")
            }
        );
    }

    #[test]
    fn save_rejects_unknown_key() {
        match parse(":save format=csv").unwrap_err() {
            CommandError::UnknownKey { verb, key } => {
                assert_eq!(verb, "save");
                assert_eq!(key, "format");
            }
            other => panic!("expected UnknownKey, got {other:?}"),
        }
    }

    #[test]
    fn search_accepts_q_from_to() {
        let got =
            parse(":search q=error from=2026-04-23T00:00:00Z to=2026-04-23T01:00:00Z").unwrap();
        match got {
            CommandAction::Search { q, from, to } => {
                assert_eq!(q.as_deref(), Some("error"));
                assert!(from.is_some());
                assert!(to.is_some());
            }
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn search_q_may_be_quoted() {
        let got = parse(":search q=\"hello world\"").unwrap();
        match got {
            CommandAction::Search { q, .. } => assert_eq!(q.as_deref(), Some("hello world")),
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn search_rejects_bad_time() {
        assert!(matches!(
            parse(":search from=not-rfc3339").unwrap_err(),
            CommandError::BadTime { key: "from", .. }
        ));
    }

    #[test]
    fn level_requires_name_and_toggle() {
        let got = parse(":level warn on").unwrap();
        assert_eq!(
            got,
            CommandAction::Level {
                level: Level::Warn,
                on: true
            }
        );
    }

    #[test]
    fn level_case_insensitive() {
        let got = parse(":level WARN off").unwrap();
        assert_eq!(
            got,
            CommandAction::Level {
                level: Level::Warn,
                on: false
            }
        );
    }

    #[test]
    fn level_rejects_bad_name() {
        match parse(":level banana on").unwrap_err() {
            CommandError::BadLevel(s) => assert_eq!(s, "banana"),
            other => panic!("expected BadLevel, got {other:?}"),
        }
    }

    #[test]
    fn level_rejects_bad_toggle() {
        match parse(":level warn maybe").unwrap_err() {
            CommandError::BadToggle(s) => assert_eq!(s, "maybe"),
            other => panic!("expected BadToggle, got {other:?}"),
        }
    }

    #[test]
    fn complete_p_returns_pause() {
        let c = complete(":p");
        assert!(c.matches.contains(&"pause"));
        assert_eq!(c.prefix, "pause");
    }

    #[test]
    fn complete_re_returns_resume_and_retry() {
        let c = complete(":re");
        assert!(c.matches.contains(&"resume"));
        assert!(c.matches.contains(&"retry"));
        // Longest shared prefix of "resume" and "retry" is "re".
        assert_eq!(c.prefix, "re");
    }

    #[test]
    fn complete_empty_returns_all_verbs() {
        let c = complete("");
        assert_eq!(c.matches.len(), ALL_VERBS.len());
        // Longest shared prefix of all 9 verbs is "".
        assert_eq!(c.prefix, "");
    }

    #[test]
    fn complete_no_match_returns_empty_list() {
        let c = complete(":zzz");
        assert!(c.matches.is_empty());
        assert_eq!(c.prefix, "");
    }

    #[test]
    fn search_unclosed_quote_errors() {
        match parse(":search q=\"foo").unwrap_err() {
            CommandError::UnterminatedQuote(s) => assert!(s.contains("q=\"foo")),
            other => panic!("expected UnterminatedQuote, got {other:?}"),
        }
    }
}
