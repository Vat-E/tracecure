//! Single source of truth for the help-overlay display.
//!
//! Note: the `key_label` strings (e.g. `"j / ↓"`, `"I W E F"`) are
//! human-readable for the help screen — they are NOT parseable as
//! key bindings. The keyboard dispatcher (T20) keeps a separate
//! `(KeyboardEvent.key, Action)` lookup map; both should be edited
//! together when keybindings change.

pub const KEYBIND_TABLE: &[(&str, &[(&str, &str)])] = &[
    (
        "Global",
        &[
            ("?", "toggle help"),
            (
                "Esc",
                "close overlay / collapse detail / leave the query box",
            ),
            (":", "open command palette"),
            ("/", "focus query input"),
            ("t", "toggle absolute / relative timestamps"),
        ],
    ),
    (
        "Stream",
        &[
            ("j / ↓", "next row (pauses the tail)"),
            ("k / ↑", "previous row (pauses the tail)"),
            ("g / Home", "oldest loaded record"),
            ("G / End", "jump to latest + resume live"),
            ("Enter", "toggle detail for selected row"),
            ("Space", "pause / resume"),
            ("r", "force reconnect"),
            ("I W E F", "toggle Info / Warn / Error / Fatal"),
            ("D T", "toggle Debug / Trace"),
        ],
    ),
    (
        "Query box",
        &[
            ("text", "filter loaded records as you type (case-sensitive)"),
            ("Enter", "search the whole time window on the server"),
            ("level:error,fatal", "only these levels"),
            (
                "app:billing  app:pay*",
                "only this app / apps with this prefix",
            ),
            ("host:edge-1  stream:api-prod", "only this host / stream"),
        ],
    ),
    (
        "Command palette",
        &[
            (":pause / :resume / :clear / :retry", ""),
            (":save path=<file>", "download buffer as NDJSON"),
            (
                ":search q=<str> from=<iso> to=<iso>",
                "server search, optional fixed window",
            ),
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
    fn table_has_the_expected_sections() {
        let titles: Vec<&str> = KEYBIND_TABLE.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            titles,
            vec!["Global", "Stream", "Query box", "Command palette"]
        );
    }

    #[test]
    fn every_section_has_at_least_one_entry() {
        for (title, entries) in KEYBIND_TABLE {
            assert!(!entries.is_empty(), "section '{title}' is empty");
        }
    }

    #[test]
    fn global_has_colon_and_slash_and_no_browser_reserved_chords() {
        let (_, global) = KEYBIND_TABLE
            .iter()
            .find(|(t, _)| *t == "Global")
            .expect("Global section exists");
        let keys: Vec<&str> = global.iter().map(|(k, _)| *k).collect();
        // Ctrl+C is copy and Ctrl+T is "new tab": the browser acts on both
        // before the page sees them, so neither can be a binding here.
        assert!(!keys.iter().any(|k| k.starts_with("Ctrl+")));
        assert!(keys.contains(&":"));
        assert!(keys.contains(&"/"));
    }
}
