//! Bottom status strip — connection, what is loaded, and which window and
//! time zone the list is in. Non-interactive.

use leptos::prelude::*;
use tracecure_ui::state::Status;
use tracecure_ui::timefmt::zone_label;

use crate::app::{AppCtx, TzCtx};

#[component]
pub fn StatusBar() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;
    let tz = expect_context::<TzCtx>().0;

    let status = move || state.with(|s| s.status);

    let status_text = move || match status() {
        Status::Connected => ("connected".to_string(), "ok"),
        Status::Connecting => ("connecting…".to_string(), "connecting"),
        Status::Disconnected => ("disconnected".to_string(), "err"),
    };

    // What the buffer is, in one phrase. The list can only ever show what
    // was loaded, so say what that was whenever it is less than the window.
    let loaded_text = move || {
        state.with(|s| {
            if s.loading {
                return "loading…".to_string();
            }
            let n = s.records.len();
            let what = if s.loaded.text.is_empty() {
                format!("{n} loaded")
            } else {
                format!("{n} loaded matching “{}”", s.loaded.text)
            };
            if s.loaded.truncated {
                format!("{what} (newest only)")
            } else {
                what
            }
        })
    };

    let needs_search = move || state.with(|s| !s.loading && s.needs_server_search());

    view! {
        <div class="status">
            {move || {
                let (txt, klass) = status_text();
                view! {
                    <span class=format!("status-dot {klass}")>{txt}</span>
                }
            }}
            <span class="dot">"·"</span>
            <span>{loaded_text}</span>
            <span class="dot">"·"</span>
            <span>{move || state.with(|s| s.time_window.label())}</span>
            <span class="dot">"·"</span>
            <span title="Times are shown in your browser's time zone">{zone_label(tz)}</span>
            <Show when=needs_search>
                <span class="dot">"·"</span>
                <span class="status-hint">
                    "filtering loaded records — "<span class="kbd">"Enter"</span>" searches the whole range"
                </span>
            </Show>
            <span class="spacer"></span>
            <span class="status-help">"press "<span class="kbd">"?"</span>" for keyboard shortcuts"</span>
        </div>
    }
}
