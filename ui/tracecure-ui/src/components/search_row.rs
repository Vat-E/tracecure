//! Bottom search / filter input. `/` focuses; `:commands` button
//! opens the command palette.

use leptos::prelude::*;
use tracecure_ui::state::AppState;

use crate::app::AppCtx;

#[component]
pub fn SearchRow() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let input_ref = NodeRef::<leptos::html::Input>::new();

    let on_input = move |e: leptos::ev::Event| {
        let value = event_target_value(&e);
        state.update(|s| s.query = value);
    };

    view! {
        <div class="search-row">
            <div class="query">
                <span class="ic">"⌕"</span>
                <input type="text"
                       placeholder="search / filter — e.g. level:error app:api-prod duration>5s"
                       node_ref=input_ref
                       on:input=on_input/>
                <span class="kbd">"/"</span>
            </div>
            <div class="cmd"
                 on:click=move |_| state.update(AppState::open_command_prompt)>
                <span class="kbd">":"</span>"commands"
            </div>
        </div>
    }
}
