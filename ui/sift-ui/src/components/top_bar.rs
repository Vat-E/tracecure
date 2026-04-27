//! Top bar — time range picker, level pills, line count, LIVE button.

use leptos::prelude::*;
use sift_core::Level;
use sift_ui::state::AppState;

use crate::app::AppCtx;

#[component]
pub fn TopBar() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    // Signals derived from state for reactivity.
    let lines = move || state.with(|s| s.records.len());
    let live = move || state.with(|s| s.live);
    let level_set = move || state.with(|s| s.level_set);

    view! {
        <div class="top">
            <div class="range">
                <span class="ic">"◷"</span>
                <span>"Last 1 h"</span>
                <span class="chev">"▾"</span>
            </div>
            <div class="levels">
                {move || level_pill("I", Level::Info, level_set().info, "var(--ok)", state)}
                {move || level_pill("W", Level::Warn, level_set().warn, "var(--warn)", state)}
                {move || level_pill("E", Level::Error, level_set().error, "var(--error)", state)}
                {move || level_pill("F", Level::Fatal, level_set().fatal, "var(--fatal)", state)}
                {move || level_pill("D", Level::Debug, level_set().debug, "var(--debug)", state)}
                {move || level_pill("T", Level::Trace, level_set().trace, "var(--text-dim)", state)}
            </div>
            <div class="spacer"></div>
            <div class="lines">
                <span class="n">{move || format_commas(lines())}</span>
                <span class="u">"lines"</span>
            </div>
            <div class="live-btn"
                 on:click=move |_| state.update(|s| if s.live { s.live = false } else { s.resume_live() })>
                <span class="d"></span>
                {move || if live() { "LIVE" } else { "PAUSED" }}
            </div>
        </div>
    }
}

fn level_pill(
    letter: &'static str,
    lvl: Level,
    on: bool,
    color_var: &'static str,
    state: RwSignal<AppState>,
) -> impl IntoView {
    let class = if on { "lvl-pill on" } else { "lvl-pill off" };
    let style = if on {
        format!("color:{color_var}")
    } else {
        String::new()
    };
    view! {
        <div class=class style=style
             on:click=move |_| state.update(|s| s.toggle_level(lvl))>
            <span class="d" style=format!("background:{color_var}")></span>
            {letter}
        </div>
    }
}

fn format_commas(n: usize) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        let from_end = bytes.len() - i;
        if i > 0 && from_end.is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_commas_basic() {
        assert_eq!(format_commas(0), "0");
        assert_eq!(format_commas(999), "999");
        assert_eq!(format_commas(1_000), "1,000");
        assert_eq!(format_commas(12_438), "12,438");
        assert_eq!(format_commas(1_234_567), "1,234,567");
    }
}
