//! Left icon rail — full height. Brand + Logs (active) + Help.
//!
//! There is no Settings entry: there is nothing to set yet, and an icon
//! that takes a click and does nothing is worse than no icon.

use leptos::prelude::*;

use crate::app::{AppCtx, UserCtx};
use tracecure_ui::state::AppState;

#[component]
pub fn Rail() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let user = expect_context::<UserCtx>().0;
    view! {
        <nav class="rail island" aria-label="Main">
            <div class="brand" aria-hidden="true">"t"</div>
            <div class="icon active" title="Logs" aria-current="page">"☰"</div>
            <div class="spacer"></div>
            // Only when the server requires sign-in: otherwise there is no
            // session to end.
            <Show when=move || user.with(Option::is_some)>
                <button type="button" class="icon"
                        title=move || format!("Sign out ({})", user.get().unwrap_or_default())
                        aria-label="Sign out"
                        on:click=|_| crate::auth_client::sign_out()>
                    "⏻"
                </button>
            </Show>
            <button type="button" class="icon" title="Keyboard shortcuts (?)"
                    aria-label="Keyboard shortcuts"
                    on:click=move |_| ctx.0.update(AppState::toggle_help)>
                "?"
            </button>
        </nav>
    }
}
