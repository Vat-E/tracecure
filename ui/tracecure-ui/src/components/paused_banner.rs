//! Paused banner — fixed floating pill, visible only when !live.

use leptos::prelude::*;
use tracecure_ui::state::AppState;

use crate::app::AppCtx;

#[component]
pub fn PausedBanner() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let live = move || state.with(|s| s.live);
    let pending = move || state.with(|s| s.pending_count);

    view! {
        <Show when=move || !live()>
            <button type="button" class="resume"
                    on:click=move |_| state.update(AppState::resume_live)>
                <span class="d" aria-hidden="true"></span>
                <span>
                    "Paused · "
                    <span class="n">{pending}</span>
                    {move || if pending() == 1 { " new record" } else { " new records" }}
                </span>
                <span class="play">"▶ Resume live"</span>
            </button>
        </Show>
    }
}
