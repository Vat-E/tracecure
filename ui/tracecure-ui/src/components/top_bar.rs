//! Top bar — time range picker, level pills, line count, LIVE button.

use leptos::prelude::*;
use sift_core::Level;
use sift_ui::state::AppState;
use sift_ui::state::TimeWindow;

use crate::app::AppCtx;

#[component]
pub fn TopBar() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;
    let range_open = RwSignal::new(false);

    // Signals derived from state for reactivity.
    let lines = move || state.with(|s| s.records.len());
    let live = move || state.with(|s| s.live);
    let level_set = move || state.with(|s| s.level_set);

    view! {
        <div class="top">
            <div class="range-wrap">
                <div class="range"
                     on:click=move |_| range_open.update(|o| *o = !*o)>
                    <span class="ic">"◷"</span>
                    <span>{move || window_label(state)}</span>
                    <span class="chev">"▾"</span>
                </div>
                <Show when=move || range_open.get()>
                    <div class="range-dropdown">
                        {preset_item("Last 15 min", 900, state, range_open)}
                        {preset_item("Last 1 h", 3600, state, range_open)}
                        {preset_item("Last 6 h", 21_600, state, range_open)}
                        {preset_item("Last 24 h", 86_400, state, range_open)}
                        {preset_item("Last 7 d", 604_800, state, range_open)}
                    </div>
                </Show>
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

fn window_label(state: RwSignal<AppState>) -> String {
    state.with(|s| match &s.time_window {
        TimeWindow::Preset(d) => {
            let secs = d.as_secs();
            if secs == 900 {
                "Last 15 min".to_string()
            } else if secs == 3600 {
                "Last 1 h".to_string()
            } else if secs == 21_600 {
                "Last 6 h".to_string()
            } else if secs == 86_400 {
                "Last 24 h".to_string()
            } else if secs == 604_800 {
                "Last 7 d".to_string()
            } else {
                format!("Last {secs}s")
            }
        }
        TimeWindow::Custom { from, to } => {
            format!(
                "{} → {}",
                from.format("%m-%d %H:%M"),
                to.format("%m-%d %H:%M")
            )
        }
    })
}

fn preset_item(
    label: &'static str,
    secs: u64,
    state: RwSignal<AppState>,
    open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="range-item"
             on:click=move |_| {
                 state.update(|s| s.time_window = TimeWindow::Preset(std::time::Duration::from_secs(secs)));
                 open.set(false);
             }>
            {label}
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
