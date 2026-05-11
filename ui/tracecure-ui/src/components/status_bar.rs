//! Bottom status strip — 24px tall, non-interactive info line.

use leptos::prelude::*;
use sift_ui::state::Status;

use crate::app::AppCtx;

#[component]
pub fn StatusBar() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let status = move || state.with(|s| s.status);
    let records = move || state.with(|s| s.records.len());

    let status_text = move || match status() {
        Status::Connected => ("connected".to_string(), "ok"),
        Status::Connecting => ("connecting…".to_string(), "connecting"),
        Status::Disconnected => ("disconnected".to_string(), "err"),
    };

    view! {
        <div class="status">
            {move || {
                let (txt, klass) = status_text();
                view! {
                    <span class=format!("status-dot {klass}")>{txt}</span>
                }
            }}
            <span class="dot">"·"</span>
            <span>{move || format!("{} records", records())}</span>
            <span class="dot">"·"</span>
            <span>"window: Last 1 h"</span>
            <span class="spacer"></span>
            <span>"press "<span class="kbd">"?"</span>" for keyboard shortcuts"</span>
        </div>
    }
}
