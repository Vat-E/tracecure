//! Stream — scrollable log container. One <Row> per filtered record.
//! Fires `on_scroll_not_at_bottom` when user scrolls away from bottom.

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
                             s.at_bottom = true;
                             // note: do NOT auto-resume live (hybrid C)
                         } else {
                             // scroll up detected
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
