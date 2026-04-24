//! Pane focus — which bordered region is currently receiving input.

use super::state::Mode;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Pane {
    // Tail-mode panes
    Records,
    Filter,
    LevelsBar,
    RateBar, // non-focusable; excluded from cycle

    // Search-mode panes
    SearchQuery,
    SearchFrom,
    SearchTo,
    SearchResults,

    // Overlays (locked while open)
    DetailPopup,
    HelpPopup,
    CommandPrompt,
}

/// Order used for Tab/Shift+Tab cycling. `RateBar` is display-only — skipped.
const TAIL_CYCLE: &[Pane] = &[Pane::Records, Pane::Filter, Pane::LevelsBar];
const SEARCH_CYCLE: &[Pane] = &[
    Pane::SearchQuery,
    Pane::SearchFrom,
    Pane::SearchTo,
    Pane::SearchResults,
    Pane::LevelsBar,
];

#[must_use]
pub fn cycle(current: Pane, mode: Mode, forward: bool) -> Pane {
    // Overlays lock focus — cycling is a no-op.
    if matches!(
        current,
        Pane::DetailPopup | Pane::HelpPopup | Pane::CommandPrompt
    ) {
        return current;
    }

    let ring: &[Pane] = match mode {
        Mode::Tail => TAIL_CYCLE,
        Mode::Search => SEARCH_CYCLE,
    };
    let Some(idx) = ring.iter().position(|p| *p == current) else {
        // Current pane isn't in this mode's cycle (e.g. user just swapped
        // modes). Snap to the first entry of the new cycle.
        return ring[0];
    };
    let next = if forward {
        (idx + 1) % ring.len()
    } else {
        (idx + ring.len() - 1) % ring.len()
    };
    ring[next]
}

/// Default pane when a mode is first entered.
#[must_use]
pub fn default_pane(mode: Mode) -> Pane {
    match mode {
        Mode::Tail => Pane::Records,
        Mode::Search => Pane::SearchQuery,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_cycle_forward_hits_every_non_rate_pane() {
        let seq: Vec<Pane> =
            std::iter::successors(Some(Pane::Records), |p| Some(cycle(*p, Mode::Tail, true)))
                .take(4)
                .collect();
        // Records → Filter → LevelsBar → Records
        assert_eq!(
            seq,
            vec![Pane::Records, Pane::Filter, Pane::LevelsBar, Pane::Records]
        );
    }

    #[test]
    fn tail_cycle_backward_is_reverse() {
        let seq: Vec<Pane> =
            std::iter::successors(Some(Pane::Records), |p| Some(cycle(*p, Mode::Tail, false)))
                .take(4)
                .collect();
        assert_eq!(
            seq,
            vec![Pane::Records, Pane::LevelsBar, Pane::Filter, Pane::Records]
        );
    }

    #[test]
    fn search_cycle_includes_all_five_panes() {
        let seq: Vec<Pane> = std::iter::successors(Some(Pane::SearchQuery), |p| {
            Some(cycle(*p, Mode::Search, true))
        })
        .take(6)
        .collect();
        assert_eq!(
            seq,
            vec![
                Pane::SearchQuery,
                Pane::SearchFrom,
                Pane::SearchTo,
                Pane::SearchResults,
                Pane::LevelsBar,
                Pane::SearchQuery,
            ]
        );
    }

    #[test]
    fn overlay_focus_does_not_cycle() {
        assert_eq!(
            cycle(Pane::DetailPopup, Mode::Tail, true),
            Pane::DetailPopup
        );
        assert_eq!(cycle(Pane::HelpPopup, Mode::Tail, true), Pane::HelpPopup);
        assert_eq!(
            cycle(Pane::CommandPrompt, Mode::Search, false),
            Pane::CommandPrompt
        );
    }

    #[test]
    fn cycle_from_wrong_mode_snaps_to_first() {
        // A SearchQuery while in Tail mode should jump to Records.
        assert_eq!(cycle(Pane::SearchQuery, Mode::Tail, true), Pane::Records);
    }

    #[test]
    fn default_pane_per_mode() {
        assert_eq!(default_pane(Mode::Tail), Pane::Records);
        assert_eq!(default_pane(Mode::Search), Pane::SearchQuery);
    }
}
