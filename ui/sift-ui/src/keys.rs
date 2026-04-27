//! Single source of truth for the help-overlay display.
//!
//! Note: the `key_label` strings (e.g. `"j / ↓"`, `"I W E F"`) are
//! human-readable for the help screen — they are NOT parseable as
//! key bindings. The keyboard dispatcher (T20) keeps a separate
//! `(KeyboardEvent.key, Action)` lookup map; both should be edited
//! together when keybindings change.

// TODO(T16/T20): remove this allow once the help modal or keyboard
// dispatcher imports KEYBIND_TABLE.
#[allow(dead_code)]
pub const KEYBIND_TABLE: &[(&str, &[(&str, &str)])] = &[
    (
        "Global",
        &[
            ("Ctrl+C", "quit / blur the page"),
            ("?", "toggle help"),
            ("Esc", "close overlay / collapse detail"),
            (":", "open command palette"),
            ("/", "focus query input"),
            ("Ctrl+T", "toggle absolute / relative timestamps"),
        ],
    ),
    (
        "Stream",
        &[
            ("j / ↓", "next row"),
            ("k / ↑", "previous row"),
            ("g / Home", "top of window"),
            ("G / End", "jump to latest + resume live"),
            ("Enter", "toggle detail for selected row"),
            ("Space", "pause / resume"),
            ("r", "force reconnect"),
            ("I W E F", "toggle Info / Warn / Error / Fatal"),
            ("D T", "toggle Debug / Trace"),
        ],
    ),
    (
        "Command palette",
        &[
            (":pause / :resume / :clear / :retry", ""),
            (":save path=<file>", "download buffer as NDJSON"),
            (":search q=<str> from=<iso> to=<iso>", ""),
            (":level <name> on|off", ""),
            (":help / :quit", ""),
            ("Tab", "complete verb prefix"),
            ("Esc", "cancel"),
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_three_sections() {
        let titles: Vec<&str> = KEYBIND_TABLE.iter().map(|(t, _)| *t).collect();
        assert_eq!(titles, vec!["Global", "Stream", "Command palette"]);
    }

    #[test]
    fn every_section_has_at_least_one_entry() {
        for (title, entries) in KEYBIND_TABLE {
            assert!(!entries.is_empty(), "section '{title}' is empty");
        }
    }

    #[test]
    fn global_has_ctrl_c_and_colon_and_slash() {
        let (_, global) = KEYBIND_TABLE
            .iter()
            .find(|(t, _)| *t == "Global")
            .expect("Global section exists");
        let keys: Vec<&str> = global.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"Ctrl+C"));
        assert!(keys.contains(&":"));
        assert!(keys.contains(&"/"));
    }
}
