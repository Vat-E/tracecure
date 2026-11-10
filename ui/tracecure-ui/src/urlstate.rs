//! View state carried in the page URL, so a view can be bookmarked or sent
//! to someone: `/?q=level%3Aerror%20timeout&w=21600&lv=ef`.
//!
//! Pure string work — the wasm side only reads `location.search` and calls
//! `history.replaceState`. Anything at its default is left out, so the
//! default view is a bare `/`.

use crate::state::LevelSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlState {
    pub query: String,
    /// Preset window length in seconds. `None` = the default window.
    pub window_secs: Option<u64>,
    /// `None` = the default level set.
    pub levels: Option<LevelSet>,
}

impl UrlState {
    /// Parse a `location.search` string (with or without the leading `?`).
    /// Unknown keys and malformed values are ignored rather than rejected:
    /// a stale or hand-edited link should still open the app.
    #[must_use]
    pub fn parse(search: &str) -> Self {
        let mut out = Self {
            query: String::new(),
            window_secs: None,
            levels: None,
        };
        for pair in search.trim_start_matches('?').split('&') {
            let Some((k, v)) = pair.split_once('=') else {
                continue;
            };
            match k {
                "q" => out.query = decode(v),
                "w" => out.window_secs = v.parse::<u64>().ok().filter(|s| *s > 0),
                "lv" => out.levels = Some(levels_from_letters(v)),
                _ => {}
            }
        }
        out
    }

    /// The `location.search` string for this state: empty, or `?…`.
    #[must_use]
    pub fn to_search(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.query.is_empty() {
            parts.push(format!("q={}", encode(&self.query)));
        }
        if let Some(w) = self.window_secs {
            parts.push(format!("w={w}"));
        }
        if let Some(l) = self.levels {
            parts.push(format!("lv={}", levels_to_letters(l)));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// One letter per enabled level, in severity order: `t d i w e f`.
#[must_use]
pub fn levels_to_letters(l: LevelSet) -> String {
    let mut s = String::new();
    for (on, c) in [
        (l.trace, 't'),
        (l.debug, 'd'),
        (l.info, 'i'),
        (l.warn, 'w'),
        (l.error, 'e'),
        (l.fatal, 'f'),
    ] {
        if on {
            s.push(c);
        }
    }
    s
}

#[must_use]
pub fn levels_from_letters(s: &str) -> LevelSet {
    let has = |c: char| s.contains(c);
    LevelSet {
        trace: has('t'),
        debug: has('d'),
        info: has('i'),
        warn: has('w'),
        error: has('e'),
        fatal: has('f'),
    }
}

fn encode(s: &str) -> String {
    const KEEP: &[u8] = b"-_.~";
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || KEEP.contains(b) {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Value of an ASCII hex digit. Callers check `is_ascii_hexdigit` first.
fn hex(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        _ => b - b'A' + 10,
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit() =>
            {
                out.push(hex(bytes[i + 1]) << 4 | hex(bytes[i + 2]));
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_a_bare_url() {
        let s = UrlState {
            query: String::new(),
            window_secs: None,
            levels: None,
        };
        assert_eq!(s.to_search(), "");
        assert_eq!(UrlState::parse(""), s);
    }

    #[test]
    fn roundtrips_query_window_and_levels() {
        let s = UrlState {
            query: "level:error app:billing-* connection timeout 50%".into(),
            window_secs: Some(21_600),
            levels: Some(LevelSet {
                info: false,
                warn: false,
                error: true,
                fatal: true,
                debug: false,
                trace: false,
            }),
        };
        let search = s.to_search();
        assert!(!search.contains(' '), "must be URL-safe: {search}");
        assert_eq!(UrlState::parse(&search), s);
    }

    #[test]
    fn non_ascii_query_survives() {
        let s = UrlState {
            query: "ошибка оплаты".into(),
            window_secs: None,
            levels: None,
        };
        assert_eq!(UrlState::parse(&s.to_search()).query, "ошибка оплаты");
    }

    #[test]
    fn garbage_is_ignored_not_fatal() {
        let s = UrlState::parse("?w=abc&lv=&zzz=1&novalue&q=%ZZ%4");
        assert_eq!(s.window_secs, None);
        assert_eq!(s.levels, Some(levels_from_letters("")));
        assert_eq!(s.query, "%ZZ%4");
    }

    #[test]
    fn plus_decodes_to_space() {
        assert_eq!(UrlState::parse("?q=a+b").query, "a b");
    }

    #[test]
    fn level_letters_roundtrip() {
        let l = LevelSet::defaults();
        assert_eq!(levels_to_letters(l), "iwef");
        assert_eq!(levels_from_letters("iwef"), l);
    }
}
