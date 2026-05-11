//! One log row — grid columns: date / host / app / lvl / body. Click
//! to expand inline detail.

use leptos::prelude::*;
use sift_core::{Level, Record};
use sift_ui::state::TimestampMode;

use crate::app::AppCtx;
use crate::components::row_detail::RowDetail;

#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn Row(idx: usize, record: Record) -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let is_selected = move || state.with(|s| s.selected_row_index == Some(idx));
    let is_expanded =
        move || state.with(|s| s.detail_expanded && s.selected_row_index == Some(idx));
    let ts_mode = move || state.with(|s| s.timestamp_mode);

    let level_class = match record.level {
        Level::Info | Level::Trace => "row",
        Level::Warn => "row warn",
        Level::Error => "row error",
        Level::Fatal => "row fatal",
        Level::Debug => "row debug",
    };

    let row_class = move || {
        if is_selected() {
            format!("{level_class} selected")
        } else {
            level_class.to_string()
        }
    };

    let record_for_detail = record.clone();
    let record_for_ts = record.clone();
    let record_for_row = record.clone();

    view! {
        <>
            <div class=row_class
                 on:click=move |_| state.update(|s| {
                     if s.selected_row_index == Some(idx) && s.detail_expanded {
                         s.close_detail();
                     } else {
                         s.open_detail(idx);
                     }
                 })>
                <span class="date">{move || format_ts(&record_for_ts.timestamp, ts_mode())}</span>
                <span class="host">{record_for_row.host.clone()}</span>
                <span class="app">{record_for_row.app.clone()}</span>
                <span class="lvl">{level_str(record_for_row.level)}</span>
                <span class="body">{record_for_row.body.clone()}</span>
            </div>
            <Show when=is_expanded>
                <RowDetail record=record_for_detail.clone()/>
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
