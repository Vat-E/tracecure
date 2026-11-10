//! Left icon rail — full height. Brand + Logs (active) + Help.
//!
//! There is no Settings entry: there is nothing to set yet, and an icon
//! that takes a click and does nothing is worse than no icon.

use leptos::prelude::*;

use crate::app::AppCtx;
use tracecure_ui::state::AppState;

#[component]
pub fn Rail() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    view! {
        <nav class="rail" aria-label="Main">
            <div class="brand" aria-hidden="true">"t"</div>
            <div class="icon active" title="Logs" aria-current="page">"☰"</div>
            <div class="spacer"></div>
            <button type="button" class="icon" title="Keyboard shortcuts (?)"
                    aria-label="Keyboard shortcuts"
                    on:click=move |_| ctx.0.update(AppState::toggle_help)>
                "?"
            </button>
        </nav>
    }
}
