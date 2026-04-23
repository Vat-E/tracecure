//! Pretty-print + colorize a `serde_json::Value` into `Vec<Line<'static>>`
//! for Ratatui. Pure function, no I/O.

// Task 9 scaffolding: `highlight` is consumed by later UX-v2 tasks that wire
// the detail panel into the TUI; exercised here only by unit tests.
#![allow(dead_code)]

use std::fmt::Write as _;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

/// Maximum recursion depth. TUI logs come from arbitrary external sources;
/// deeply-nested payloads should display a truncation marker rather than
/// crash the TUI.
const DEPTH_LIMIT: usize = 128;

pub fn highlight(v: &Value) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    walk(v, 0, &mut lines, &mut current);
    if !current.is_empty() {
        lines.push(Line::from(std::mem::take(&mut current)));
    }
    lines
}

fn walk(
    v: &Value,
    indent: usize,
    lines: &mut Vec<Line<'static>>,
    current: &mut Vec<Span<'static>>,
) {
    if indent > DEPTH_LIMIT * 2 {
        // `indent` grows by 2 per level, so DEPTH_LIMIT*2 = max indent.
        current.push(punct("…"));
        return;
    }
    match v {
        Value::Null => current.push(scalar("null")),
        Value::Bool(b) => current.push(scalar(if *b { "true" } else { "false" }.to_string())),
        Value::Number(n) => current.push(scalar(n.to_string())),
        Value::String(s) => current.push(string_span(format!("\"{}\"", escape(s)))),
        Value::Array(arr) => walk_array(arr, indent, lines, current),
        Value::Object(map) => walk_object(map, indent, lines, current),
    }
}

fn walk_array(
    arr: &[Value],
    indent: usize,
    lines: &mut Vec<Line<'static>>,
    current: &mut Vec<Span<'static>>,
) {
    if arr.is_empty() {
        current.push(punct("[]"));
        return;
    }
    current.push(punct("["));
    flush(lines, current);

    let inner = indent + 2;
    let last = arr.len() - 1;
    for (i, v) in arr.iter().enumerate() {
        current.push(Span::raw(" ".repeat(inner)));
        walk(v, inner, lines, current);
        if i != last {
            current.push(punct(","));
        }
        flush(lines, current);
    }
    current.push(Span::raw(" ".repeat(indent)));
    current.push(punct("]"));
}

fn walk_object(
    map: &serde_json::Map<String, Value>,
    indent: usize,
    lines: &mut Vec<Line<'static>>,
    current: &mut Vec<Span<'static>>,
) {
    if map.is_empty() {
        current.push(punct("{}"));
        return;
    }
    current.push(punct("{"));
    flush(lines, current);

    let inner = indent + 2;
    let last = map.len() - 1;
    for (i, (k, v)) in map.iter().enumerate() {
        current.push(Span::raw(" ".repeat(inner)));
        current.push(key_span(format!("\"{}\"", escape(k))));
        current.push(punct(": "));
        walk(v, inner, lines, current);
        if i != last {
            current.push(punct(","));
        }
        flush(lines, current);
    }
    current.push(Span::raw(" ".repeat(indent)));
    current.push(punct("}"));
}

fn flush(lines: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>) {
    lines.push(Line::from(std::mem::take(current)));
}

fn key_span(s: String) -> Span<'static> {
    Span::styled(s, Style::default().fg(Color::Cyan))
}

fn string_span(s: String) -> Span<'static> {
    Span::styled(s, Style::default().fg(Color::Green))
}

fn scalar(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::Yellow))
}

fn punct(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::default().fg(Color::DarkGray))
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{08}' => o.push_str("\\b"),
            '\u{0c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                // Writing to String is infallible.
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line_text(ln: &Line<'static>) -> String {
        ln.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn null_one_line() {
        let lines = highlight(&Value::Null);
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "null");
    }

    #[test]
    fn empty_object_one_line() {
        let lines = highlight(&json!({}));
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "{}");
    }

    #[test]
    fn single_key_object_is_three_lines() {
        let lines = highlight(&json!({"user": "alice"}));
        // Expected:
        // line 0: {
        // line 1:   "user": "alice"
        // line 2: }
        assert_eq!(lines.len(), 3);
        assert_eq!(line_text(&lines[0]), "{");
        assert_eq!(line_text(&lines[1]), "  \"user\": \"alice\"");
        assert_eq!(line_text(&lines[2]), "}");
    }

    #[test]
    fn nested_object_indents_two_per_level() {
        let lines = highlight(&json!({"a": {"b": 1}}));
        // line 2 should be "    \"b\": 1"
        let found = lines.iter().any(|l| line_text(l).starts_with("    \"b\""));
        assert!(
            found,
            "expected nested \"b\" key at 4-space indent; got {:?}",
            lines.iter().map(line_text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn key_span_is_cyan_scalar_is_yellow_string_is_green() {
        let lines = highlight(&json!({"n": 42, "s": "hi"}));
        // Find cyan "n", yellow 42, green "hi"
        let all_spans: Vec<&Span> = lines.iter().flat_map(|l| l.spans.iter()).collect();
        assert!(all_spans
            .iter()
            .any(|s| s.content.as_ref() == "\"n\"" && s.style.fg == Some(Color::Cyan)));
        assert!(all_spans
            .iter()
            .any(|s| s.content.as_ref() == "42" && s.style.fg == Some(Color::Yellow)));
        assert!(all_spans
            .iter()
            .any(|s| s.content.as_ref() == "\"hi\"" && s.style.fg == Some(Color::Green)));
    }

    #[test]
    fn array_of_scalars_one_item_per_line() {
        let lines = highlight(&json!([1, "two", false]));
        // [, 1, ,, "two", ,, false, ]
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert!(texts.contains(&"[".to_string()));
        assert!(texts.iter().any(|t| t.contains("1,")));
        assert!(texts.iter().any(|t| t.contains("\"two\",")));
        assert!(texts.iter().any(|t| t.contains("false")));
        assert!(texts.contains(&"]".to_string()));
    }

    #[test]
    fn deeply_nested_is_truncated_not_panics() {
        let mut v = Value::Null;
        for _ in 0..200 {
            v = Value::Array(vec![v]);
        }
        let lines = highlight(&v);
        let text: String = lines.iter().map(line_text).collect();
        assert!(text.contains("…"), "expected truncation marker in: {text}");
    }
}
