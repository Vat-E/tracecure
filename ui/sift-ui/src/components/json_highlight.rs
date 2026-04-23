//! Pure tokenizer for pretty-printed JSON. Produces `(TokenKind, String)` pairs
//! that the Leptos view renders as <span class="…">. No allocations that aren't
//! forced by the borrow; strings are owned because the tokenizer is used once
//! per detail-panel render.

use leptos::*;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Key,
    String,
    Number,
    Bool,
    Null,
    Punct,
    Whitespace,
}

impl TokenKind {
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Key => "json-key",
            Self::String => "json-string",
            Self::Number | Self::Bool | Self::Null => "json-scalar",
            Self::Punct => "json-punct",
            Self::Whitespace => "json-ws",
        }
    }
}

pub fn tokenize(v: &Value) -> Vec<(TokenKind, String)> {
    let mut out = Vec::new();
    walk(v, 0, &mut out);
    out
}

fn walk(v: &Value, indent: usize, out: &mut Vec<(TokenKind, String)>) {
    match v {
        Value::Null => out.push((TokenKind::Null, "null".into())),
        Value::Bool(b) => out.push((TokenKind::Bool, b.to_string())),
        Value::Number(n) => out.push((TokenKind::Number, n.to_string())),
        Value::String(s) => out.push((TokenKind::String, format!("\"{}\"", escape(s)))),
        Value::Array(arr) => walk_array(arr, indent, out),
        Value::Object(map) => walk_object(map, indent, out),
    }
}

fn walk_array(arr: &[Value], indent: usize, out: &mut Vec<(TokenKind, String)>) {
    if arr.is_empty() {
        out.push((TokenKind::Punct, "[]".into()));
        return;
    }
    out.push((TokenKind::Punct, "[".into()));
    out.push((TokenKind::Whitespace, "\n".into()));
    let inner = indent + 2;
    for (i, v) in arr.iter().enumerate() {
        out.push((TokenKind::Whitespace, " ".repeat(inner)));
        walk(v, inner, out);
        if i + 1 < arr.len() {
            out.push((TokenKind::Punct, ",".into()));
        }
        out.push((TokenKind::Whitespace, "\n".into()));
    }
    out.push((TokenKind::Whitespace, " ".repeat(indent)));
    out.push((TokenKind::Punct, "]".into()));
}

fn walk_object(
    map: &serde_json::Map<String, Value>,
    indent: usize,
    out: &mut Vec<(TokenKind, String)>,
) {
    if map.is_empty() {
        out.push((TokenKind::Punct, "{}".into()));
        return;
    }
    out.push((TokenKind::Punct, "{".into()));
    out.push((TokenKind::Whitespace, "\n".into()));
    let inner = indent + 2;
    let last = map.len() - 1;
    for (i, (k, v)) in map.iter().enumerate() {
        out.push((TokenKind::Whitespace, " ".repeat(inner)));
        out.push((TokenKind::Key, format!("\"{}\"", escape(k))));
        out.push((TokenKind::Punct, ": ".into()));
        walk(v, inner, out);
        if i != last {
            out.push((TokenKind::Punct, ",".into()));
        }
        out.push((TokenKind::Whitespace, "\n".into()));
    }
    out.push((TokenKind::Whitespace, " ".repeat(indent)));
    out.push((TokenKind::Punct, "}".into()));
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
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

#[component]
pub fn JsonHighlight(#[prop(into)] value: serde_json::Value) -> impl IntoView {
    let tokens = tokenize(&value);
    view! {
        <pre class="json">
            {tokens.into_iter().map(|(k, s)| view! {
                <span class=k.css_class()>{s}</span>
            }).collect::<Vec<_>>()}
        </pre>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kinds(v: &[(TokenKind, String)]) -> Vec<TokenKind> {
        v.iter().map(|(k, _)| *k).collect()
    }

    fn text(v: &[(TokenKind, String)]) -> String {
        v.iter().map(|(_, s)| s.as_str()).collect()
    }

    #[test]
    fn null_tokenizes_as_single_null_token() {
        let toks = tokenize(&Value::Null);
        assert_eq!(toks, vec![(TokenKind::Null, "null".into())]);
    }

    #[test]
    fn empty_object_is_one_punct_token() {
        assert_eq!(tokenize(&json!({})), vec![(TokenKind::Punct, "{}".into())]);
    }

    #[test]
    fn empty_array_is_one_punct_token() {
        assert_eq!(tokenize(&json!([])), vec![(TokenKind::Punct, "[]".into())]);
    }

    #[test]
    fn object_with_string_value_emits_key_punct_string() {
        let toks = tokenize(&json!({"user": "alice"}));
        // Expect: { \n  "user": "alice" \n }
        let ks = kinds(&toks);
        assert!(ks.contains(&TokenKind::Key));
        assert!(ks.contains(&TokenKind::String));
        assert!(ks.contains(&TokenKind::Punct));
        assert!(text(&toks).contains("\"user\""));
        assert!(text(&toks).contains("\"alice\""));
    }

    #[test]
    fn numbers_and_bools_are_scalars() {
        let toks = tokenize(&json!({"n": 42, "b": true, "x": null}));
        let ks = kinds(&toks);
        assert!(ks.contains(&TokenKind::Number));
        assert!(ks.contains(&TokenKind::Bool));
        assert!(ks.contains(&TokenKind::Null));
    }

    #[test]
    fn escape_quotes_and_newlines() {
        assert_eq!(escape("he said \"hi\"\n"), "he said \\\"hi\\\"\\n");
    }

    #[test]
    fn nested_object_indents_two_per_level() {
        let toks = tokenize(&json!({"a": {"b": 1}}));
        let s = text(&toks);
        // Inner key "b" should be indented four spaces (outer 2 + inner 2).
        assert!(s.contains("    \"b\""), "got: {s}");
    }
}
