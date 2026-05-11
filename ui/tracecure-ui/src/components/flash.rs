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

    // Tick effect: every 500ms, call expire_flash_if_due. Spawned once
    // when the component mounts.
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                TimeoutFuture::new(500).await;
                state.update(AppState::expire_flash_if_due);
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
