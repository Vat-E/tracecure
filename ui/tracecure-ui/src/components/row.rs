//! One log row — grid columns: date / host / app / lvl / body. Click
//! to expand inline detail.
//!
//! This component is instantiated up to `RECORD_CAP` times, so what it
//! subscribes to matters more than what it renders. It deliberately *reads*
//! only `SelectionCtx`, `ViewCtx` and `HighlightCtx` — never `AppCtx` — so
//! that an arriving log record does not wake a closure in every mounted row.
//! (It does *write* to `AppCtx` on click, to pause the tail; a write takes
//! no subscription.)

use leptos::prelude::*;
use tracecure_core::Level;
use tracecure_ui::filter::split_matches;
use tracecure_ui::state::{AppState, Selection, TimestampMode};
use tracecure_ui::timefmt::row_time;

use crate::app::{AppCtx, ClockCtx, HighlightCtx, SelectionCtx, TzCtx, ViewCtx};
use crate::components::row_detail::RowDetail;
use tracecure_ui::state::Entry;

#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn Row(entry: Entry) -> impl IntoView {
    let app = expect_context::<AppCtx>().0;
    let selection = expect_context::<SelectionCtx>().0;
    let view = expect_context::<ViewCtx>().0;
    let clock = expect_context::<ClockCtx>().0;
    let highlight = expect_context::<HighlightCtx>().0;
    let tz = expect_context::<TzCtx>().0;

    let seq = entry.seq;
    let rec = entry.rec;

    let level_class = match rec.level {
        Level::Info | Level::Trace => "row",
        Level::Warn => "row warn",
        Level::Error => "row error",
        Level::Fatal => "row fatal",
        Level::Debug => "row debug",
    };

    let is_selected = move || selection.with(|s: &Selection| s.is_selected(seq));
    let is_expanded = move || selection.with(|s: &Selection| s.is_expanded(seq));

    let ts = rec.timestamp;
    let ts_text = move || {
        let mode = view.with(|v| v.timestamp_mode);
        if mode == TimestampMode::Relative {
            // "3m ago" is a function of the wall clock, so subscribe to the
            // tick. Leptos re-collects dependencies on every run, so a row
            // in Absolute mode never takes this dependency and an ingested
            // record still costs it nothing.
            clock.track();
        }
        row_time(&ts, mode, tz, chrono::Utc::now())
    };

    // Opening a row pauses the tail. Without that the list keeps moving and
    // the row being read scrolls away; with new records held in `pending`
    // it stays put until the user resumes.
    let on_click = move |_| {
        let opening = selection.with_untracked(|s| !s.is_expanded(seq));
        if opening && app.with_untracked(|s| s.live) {
            app.update(AppState::pause);
        }
        selection.update(|s: &mut Selection| s.toggle(seq));
    };

    let body = rec.body.clone();
    let body_view = move || {
        let term = highlight.get();
        if term.is_empty() {
            // The common case: one text node, no wrapper elements.
            return body.clone().into_any();
        }
        split_matches(&body, &term)
            .into_iter()
            .map(|(text, hit)| {
                if hit {
                    view! { <mark>{text}</mark> }.into_any()
                } else {
                    text.into_any()
                }
            })
            .collect_view()
            .into_any()
    };

    let rec_for_detail = rec.clone();

    view! {
        <>
            // `class:selected` toggles one class in place rather than
            // rebuilding the whole class string on every evaluation.
            <div class=level_class
                 class:selected=is_selected
                 on:click=on_click>
                <span class="date">{ts_text}</span>
                // `title` so a name cut off by the column is still readable.
                <span class="host" title=rec.host.clone()>{rec.host.clone()}</span>
                <span class="app" title=rec.app.clone()>{rec.app.clone()}</span>
                <span class="lvl">{level_str(rec.level)}</span>
                <span class="body">{body_view}</span>
            </div>
            <Show when=is_expanded>
                <RowDetail record=rec_for_detail.clone()/>
            </Show>
        </>
    }
}

pub fn level_str(l: Level) -> &'static str {
    match l {
        Level::Trace => "trace",
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
        Level::Fatal => "fatal",
    }
}
