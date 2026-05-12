//! Stream — scrollable log container. One <Row> per filtered record.
//!
//! Live/paused state machine:
//! - Scrolling up from the bottom → `on_scroll_not_at_bottom` → pause (live=false).
//! - Scrolling back to the bottom → `on_scroll_to_bottom` → auto-resume live
//!   (drain pending, bump `scroll_request_counter`).
//! - Clicking the Resume Live pill in `paused_banner` → `resume_live` (same
//!   path; bumps the counter).
//!
//! Both resume paths converge on `scroll_request_counter`, which an Effect
//! in this component watches to imperatively scroll the DOM container.

use leptos::prelude::*;

use crate::app::AppCtx;
use crate::components::row::Row;

const SCROLL_BOTTOM_THRESHOLD: f64 = 4.0; // px from bottom still counts as "at bottom"

#[component]
pub fn Stream() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let stream_ref = NodeRef::<leptos::html::Div>::new();

    // Reactive iteration over filtered records.
    let rows = move || {
        state.with(|s| {
            s.filtered()
                .iter()
                .enumerate()
                .map(|(idx, r)| (idx, (*r).clone()))
                .collect::<Vec<_>>()
        })
    };

    // DOM scroll on counter bump. First run captures the current counter
    // without scrolling (the mount-time position is handled naturally by
    // the browser); subsequent runs scroll to the bottom whenever the
    // counter changes (resume_live / on_scroll_to_bottom paths).
    //
    // The scroll is deferred to a microtask so that Leptos's `For`
    // reconciler has already patched any newly-drained pending rows into
    // the DOM by the time we read `scroll_height`. Without this defer the
    // Effect can race the DOM update and scroll to a stale bottom.
    Effect::new(move |last: Option<u64>| -> u64 {
        let current = state.with(|s| s.scroll_request_counter);
        if let Some(prev) = last {
            if prev != current {
                wasm_bindgen_futures::spawn_local(async move {
                    if let Some(el) = stream_ref.get() {
                        el.set_scroll_top(el.scroll_height());
                    }
                });
            }
        }
        current
    });

    view! {
        <div class="stream"
             node_ref=stream_ref
             on:scroll=move |_| {
                 if let Some(el) = stream_ref.get() {
                     let scroll_top = f64::from(el.scroll_top());
                     let client_h = f64::from(el.client_height());
                     let scroll_h = f64::from(el.scroll_height());
                     let at_bottom = (scroll_h - (scroll_top + client_h)) <= SCROLL_BOTTOM_THRESHOLD;
                     state.update(|s| {
                         if at_bottom {
                             s.on_scroll_to_bottom();
                         } else {
                             s.on_scroll_not_at_bottom();
                         }
                     });
                 }
             }>
            <For each=rows
                 key=|(i, _)| *i
                 children=move |(idx, record)| {
                     view! { <Row idx=idx record=record/> }
                 }/>
        </div>
    }
}
