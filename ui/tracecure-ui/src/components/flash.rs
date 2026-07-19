//! Flash message — floating banner above the search row.

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use tracecure_ui::state::{AppState, FlashKind};
use wasm_bindgen_futures::spawn_local;

use crate::app::AppCtx;

#[component]
pub fn Flash() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    // Tick effect: poll for an expired flash. Spawned once at mount.
    //
    // The write is gated on there actually being something to expire. An
    // unconditional `update` here notified every subscriber twice a second
    // for the lifetime of the page — a permanent idle cost paid mostly when
    // `flash` was `None`, which is nearly always.
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                TimeoutFuture::new(500).await;
                if state.with_untracked(AppState::flash_is_due) {
                    state.update(AppState::expire_flash_if_due);
                }
            }
        });
    });

    let flash = move || state.with(|s| s.flash.clone());

    view! {
        <Show when=move || flash().is_some()>
            {move || {
                let f = flash().unwrap();
                let klass = match f.kind {
                    FlashKind::Ok => "flash ok",
                    FlashKind::Err => "flash err",
                };
                view! {
                    <div class=klass>{f.msg}</div>
                }
            }}
        </Show>
    }
}
