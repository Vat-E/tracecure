//! Left icon rail — 40px wide, full height. Brand + Logs (active)
//! + Settings + Help.

use leptos::prelude::*;

use crate::app::AppCtx;
use tracecure_ui::state::AppState;

#[component]
pub fn Rail() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    view! {
        <nav class="rail">
            <div class="brand">"t"</div>
            <div class="icon active" title="Logs">"☰"</div>
            <div class="spacer"></div>
            <div class="icon" title="Settings">"⚙"</div>
            <div class="icon" title="Help · ?"
                 on:click=move |_| ctx.0.update(AppState::toggle_help)>
                "?"
            </div>
        </nav>
    }
}
