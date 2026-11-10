//! Top bar — time range picker, level pills, record count, LIVE button.

use leptos::prelude::*;
use tracecure_core::Level;
use tracecure_ui::state::AppState;
use tracecure_ui::state::TimeWindow;

use crate::app::AppCtx;

#[component]
pub fn TopBar() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;
    let range_open = RwSignal::new(false);

    // Signals derived from state for reactivity.
    let counts = move || state.with(|s| (s.visible_count(), s.records.len()));
    let live = move || state.with(|s| s.live);
    let level_set = move || state.with(|s| s.level_set);

    view! {
        <div class="top">
            <div class="top-row">
                <div class="range-wrap">
                    <button type="button" class="range"
                            aria-haspopup="menu"
                            aria-expanded=move || range_open.get().to_string()
                            on:click=move |_| range_open.update(|o| *o = !*o)>
                        <span class="ic" aria-hidden="true">"◷"</span>
                        <span>{move || state.with(|s| s.time_window.label())}</span>
                        <span class="chev" aria-hidden="true">"▾"</span>
                    </button>
                    <Show when=move || range_open.get()>
                        // A click anywhere else closes the menu.
                        <div class="range-backdrop"
                             on:click=move |_| range_open.set(false)></div>
                        <div class="range-dropdown" role="menu"
                             on:keydown=move |e| {
                                 if e.key() == "Escape" {
                                     e.stop_propagation();
                                     range_open.set(false);
                                 }
                             }>
                            {preset_item("Last 15 min", 900, state, range_open)}
                            {preset_item("Last 1 h", 3600, state, range_open)}
                            {preset_item("Last 6 h", 21_600, state, range_open)}
                            {preset_item("Last 24 h", 86_400, state, range_open)}
                            {preset_item("Last 7 d", 604_800, state, range_open)}
                        </div>
                    </Show>
                </div>
                <div class="divider"></div>
                <div class="levels" role="group" aria-label="Levels shown">
                    {move || level_pill("INFO",  Level::Info,  level_set().info,  "info",  state)}
                    {move || level_pill("WARN",  Level::Warn,  level_set().warn,  "warn",  state)}
                    {move || level_pill("ERROR", Level::Error, level_set().error, "error", state)}
                    {move || level_pill("FATAL", Level::Fatal, level_set().fatal, "fatal", state)}
                    {move || level_pill("DEBUG", Level::Debug, level_set().debug, "debug", state)}
                    {move || level_pill("TRACE", Level::Trace, level_set().trace, "trace", state)}
                </div>
                <div class="spacer"></div>
                // "shown of loaded" whenever a filter is hiding something:
                // a bare total next to a filtered list reads as its length.
                <div class="lines">
                    {move || {
                        let (shown, loaded) = counts();
                        if shown == loaded {
                            view! {
                                <span class="n">{format_commas(loaded)}</span>
                                <span class="u">"records"</span>
                            }.into_any()
                        } else {
                            view! {
                                <span class="n">{format_commas(shown)}</span>
                                <span class="u">{format!("of {} loaded", format_commas(loaded))}</span>
                            }.into_any()
                        }
                    }}
                </div>
                <button type="button" class="live-btn"
                        class:paused=move || !live()
                        title="Space"
                        aria-pressed=move || live().to_string()
                        on:click=move |_| state.update(|s| if s.live { s.pause() } else { s.resume_live() })>
                    <span class="d" aria-hidden="true"></span>
                    {move || if live() { "LIVE" } else { "PAUSED" }}
                </button>
            </div>
        </div>
    }
}

fn level_pill(
    label: &'static str,
    lvl: Level,
    on: bool,
    variant: &'static str,
    state: RwSignal<AppState>,
) -> impl IntoView {
    let class = if on {
        format!("lvl-pill on {variant}")
    } else {
        format!("lvl-pill off {variant}")
    };
    view! {
        <button type="button" class=class
                aria-pressed=on.to_string()
                on:click=move |_| state.update(|s| s.toggle_level(lvl))>
            {label}
        </button>
    }
}

fn preset_item(
    label: &'static str,
    secs: u64,
    state: RwSignal<AppState>,
    open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <button type="button" class="range-item" role="menuitem"
                on:click=move |_| {
                    state.update(|s| s.time_window = TimeWindow::Preset(std::time::Duration::from_secs(secs)));
                    open.set(false);
                }>
            {label}
        </button>
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
