//! Pure JSON tokenizer. Takes a `serde_json::Value`, produces a flat
//! `Vec<JsonToken>` where each token carries its semantic color.
//!
//! Consumers (Leptos `row_detail` component) render each token as a
//! styled `<span>` — color CSS classes map to theme variables.
//!
//! Bounded by `DEPTH_LIMIT` to avoid pathological nesting.

use serde_json::Value;
use std::fmt::Write;

pub const DEPTH_LIMIT: usize = 128;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonColor {
    Key,   // object keys
    Str,   // string values
    Num,   // number values
    Bool,  // true/false
    Null,  // null
    Punct, // { } [ ] : , and whitespace
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonToken {
    pub color: JsonColor,
    pub text: String,
}

/// Tokenize a JSON value into colored tokens. Pretty-printed output
/// (2-space indent, newlines between object/array members).
#[allow(dead_code)]
#[must_use]
pub fn tokenize(v: &Value) -> Vec<JsonToken> {
    let mut out = Vec::new();
    walk(v, &mut out, 0);
    out
}

fn walk(v: &Value, out: &mut Vec<JsonToken>, depth: usize) {
    if depth > DEPTH_LIMIT {
        out.push(JsonToken {
            color: JsonColor::Punct,
            text: "…".to_string(),
        });
        return;
    }
    match v {
        Value::Null => out.push(JsonToken {
            color: JsonColor::Null,
            text: "null".to_string(),
        }),
        Value::Bool(b) => out.push(JsonToken {
            color: JsonColor::Bool,
            text: b.to_string(),
        }),
        Value::Number(n) => out.push(JsonToken {
            color: JsonColor::Num,
            text: n.to_string(),
        }),
        Value::String(s) => {
            out.push(JsonToken {
                color: JsonColor::Str,
                text: format!("\"{}\"", escape_json_string(s)),
            });
        }
        Value::Array(arr) => {
            out.push(JsonToken {
                color: JsonColor::Punct,
                text: "[".to_string(),
            });
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(JsonToken {
                        color: JsonColor::Punct,
                        text: ", ".to_string(),
                    });
                }
                walk(item, out, depth + 1);
            }
            out.push(JsonToken {
                color: JsonColor::Punct,
                text: "]".to_string(),
            });
        }
        Value::Object(map) => {
            out.push(JsonToken {
                color: JsonColor::Punct,
                text: "{\n".to_string(),
            });
            let indent = "  ".repeat(depth + 1);
            for (i, (k, val)) in map.iter().enumerate() {
                out.push(JsonToken {
                    color: JsonColor::Punct,
                    text: indent.clone(),
                });
                out.push(JsonToken {
                    color: JsonColor::Key,
                    text: format!("\"{}\"", escape_json_string(k)),
                });
                out.push(JsonToken {
                    color: JsonColor::Punct,
                    text: ": ".to_string(),
                });
                walk(val, out, depth + 1);
                if i + 1 < map.len() {
                    out.push(JsonToken {
                        color: JsonColor::Punct,
                        text: ",".to_string(),
                    });
                }
                out.push(JsonToken {
                    color: JsonColor::Punct,
                    text: "\n".to_string(),
                });
            }
            out.push(JsonToken {
                color: JsonColor::Punct,
                text: "  ".repeat(depth) + "}",
            });
        }
    }
}

fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_null() {
        let t = tokenize(&Value::Null);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].color, JsonColor::Null);
        assert_eq!(t[0].text, "null");
    }

    #[test]
    fn primitive_bool() {
        assert_eq!(tokenize(&Value::Bool(true))[0].text, "true");
        assert_eq!(tokenize(&Value::Bool(false))[0].text, "false");
    }

    #[test]
    fn primitive_number() {
        let t = tokenize(&serde_json::json!(42));
        assert_eq!(t[0].color, JsonColor::Num);
        assert_eq!(t[0].text, "42");
    }

    #[test]
    fn primitive_string_quotes_are_included() {
        let t = tokenize(&serde_json::json!("hello"));
        assert_eq!(t[0].color, JsonColor::Str);
        assert_eq!(t[0].text, "\"hello\"");
    }

    #[test]
    fn object_has_key_then_value() {
        let v = serde_json::json!({"n": 1});
        let t = tokenize(&v);
        let key_idx = t
            .iter()
            .position(|t| t.color == JsonColor::Key)
            .expect("has a key");
        assert_eq!(t[key_idx].text, "\"n\"");
        let num_idx = t
            .iter()
            .position(|t| t.color == JsonColor::Num)
            .expect("has a number");
        assert_eq!(t[num_idx].text, "1");
    }

    #[test]
    fn array_separates_with_commas() {
        let v = serde_json::json!([1, 2, 3]);
        let t = tokenize(&v);
        let nums: Vec<&str> = t
            .iter()
            .filter(|t| t.color == JsonColor::Num)
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(nums, vec!["1", "2", "3"]);
    }

    #[test]
    fn string_escapes_are_applied() {
        let v = serde_json::json!("line1\nline2");
        let t = tokenize(&v);
        assert_eq!(t[0].text, "\"line1\\nline2\"");
    }

    #[test]
    fn depth_limit_caps_recursion() {
        // Build a deeply nested structure
        let mut v = Value::Null;
        for _ in 0..(DEPTH_LIMIT + 10) {
            v = Value::Array(vec![v]);
        }
        let t = tokenize(&v);
        // Should find the ellipsis marker somewhere
        assert!(t.iter().any(|t| t.text == "…"));
    }
}
