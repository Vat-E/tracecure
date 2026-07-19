//! One log row — grid columns: date / host / app / lvl / body. Click
//! to expand inline detail.
//!
//! This component is instantiated up to `RECORD_CAP` times, so what it
//! subscribes to matters more than what it renders. It deliberately reads
//! only `SelectionCtx` and `ViewCtx` — never `AppCtx` — so that an arriving
//! log record does not wake a closure in every mounted row.

use leptos::prelude::*;
use tracecure_core::Level;
use tracecure_ui::state::{Entry, Selection, TimestampMode};

use crate::app::{ClockCtx, SelectionCtx, ViewCtx};
use crate::components::row_detail::RowDetail;

#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn Row(entry: Entry) -> impl IntoView {
    let selection = expect_context::<SelectionCtx>().0;
    let view = expect_context::<ViewCtx>().0;
    let clock = expect_context::<ClockCtx>().0;

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
        format_ts(&ts, mode)
    };

    let rec_for_detail = rec.clone();

    view! {
        <>
            // `class:selected` toggles one class in place rather than
            // rebuilding the whole class string on every evaluation.
            <div class=level_class
                 class:selected=is_selected
                 on:click=move |_| selection.update(|s: &mut Selection| s.toggle(seq))>
                <span class="date">{ts_text}</span>
                <span class="host">{rec.host.clone()}</span>
                <span class="app">{rec.app.clone()}</span>
                <span class="lvl">{level_str(rec.level)}</span>
                <span class="body">{rec.body.clone()}</span>
            </div>
            <Show when=is_expanded>
                <RowDetail record=rec_for_detail.clone()/>
            </Show>
        </>
    }
}

fn level_str(l: Level) -> &'static str {
    match l {
        Level::Trace => "trace",
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
        Level::Fatal => "fatal",
    }
}

fn format_ts(ts: &chrono::DateTime<chrono::Utc>, mode: TimestampMode) -> String {
    match mode {
        TimestampMode::Absolute => ts.format("%H:%M:%S%.3f").to_string(),
        TimestampMode::Relative => {
            let now = chrono::Utc::now();
            let dur = now.signed_duration_since(*ts);
            let s = dur.num_seconds().max(0);
            if s < 60 {
                format!("{s}s ago")
            } else if s < 3600 {
                format!("{}m ago", s / 60)
            } else if s < 86400 {
                format!("{}h ago", s / 3600)
            } else {
                format!("{}d ago", s / 86400)
            }
        }
    }
}
