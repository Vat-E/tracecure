//! Bottom query box. `/` focuses; the `:` button opens the command palette.
//!
//! Typing filters the loaded records immediately. Enter sends the free text
//! to the server and reloads the buffer for the whole time range — the only
//! way to find something older than what is loaded. See `filter.rs` for the
//! language.

use leptos::prelude::*;
use tracecure_ui::state::{AppState, FetchAnchor};

use crate::app::{run_window_fetch, AppCtx, WINDOW_FETCH_LIMIT};

#[component]
pub fn SearchRow() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let input_ref = NodeRef::<leptos::html::Input>::new();

    // The box mirrors the state, so a query set from elsewhere — the palette,
    // an "only this" button, the URL — shows up in it. A `Memo` so the DOM
    // value is only written when the query actually changes, not on every
    // arriving record.
    let query = Memo::new(move |_| state.with(|s| s.query().to_string()));

    let set_query = move |value: String| {
        // Clearing a box whose buffer was loaded for a search would leave
        // only that search's matches on screen under an empty box. Reload.
        let reload = value.trim().is_empty()
            && state.with_untracked(|s| !s.loaded.filter.describe().is_empty());
        state.update(|s| s.set_query(value));
        if reload {
            run_window_fetch(state, WINDOW_FETCH_LIMIT, FetchAnchor::Newest);
        }
    };

    let on_input = move |e: leptos::ev::Event| set_query(event_target_value(&e));

    let on_keydown = move |e: leptos::ev::KeyboardEvent| {
        if e.key() == "Enter" {
            e.prevent_default();
            run_window_fetch(state, WINDOW_FETCH_LIMIT, FetchAnchor::Newest);
        }
    };

    let clear = move |_| {
        set_query(String::new());
        if let Some(input) = input_ref.get_untracked() {
            let _ = input.focus();
        }
    };

    view! {
        <div class="search-row">
            <div class="query">
                <span class="ic" aria-hidden="true">"⌕"</span>
                <input type="text"
                       aria-label="Filter and search"
                       placeholder="filter — e.g. level:error app:billing timeout · Enter searches the whole range"
                       spellcheck="false"
                       autocomplete="off"
                       autocapitalize="off"
                       node_ref=input_ref
                       prop:value=move || query.get()
                       on:input=on_input
                       on:keydown=on_keydown/>
                <Show when=move || !query.get().is_empty()
                      fallback=|| view! { <span class="kbd" aria-hidden="true">"/"</span> }>
                    <button type="button" class="query-clear" aria-label="Clear filter"
                            on:click=clear>
                        "×"
                    </button>
                </Show>
            </div>
            <button type="button" class="cmd" title="Command palette"
                    on:click=move |_| state.update(AppState::open_command_prompt)>
                <span class="kbd" aria-hidden="true">":"</span><span class="cmd-label">"commands"</span>
            </button>
        </div>
    }
}
