//! Help overlay (`?`). Reads from `KEYBIND_TABLE` for single source of truth.

use leptos::prelude::*;
use tracecure_ui::keys::KEYBIND_TABLE;

use crate::app::AppCtx;

#[component]
pub fn HelpModal() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;
    let open = move || state.with(|s| s.help_open);

    let sections = KEYBIND_TABLE
        .iter()
        .map(|(title, entries)| {
            let entries_view = entries
                .iter()
                .map(|(k, v)| {
                    view! {
                        <div class="help-row">
                            <span class="help-k">{*k}</span>
                            <span class="help-v">{*v}</span>
                        </div>
                    }
                })
                .collect::<Vec<_>>();
            view! {
                <div class="help-section">
                    <h3 class="help-title">{*title}</h3>
                    <div class="help-entries">{entries_view}</div>
                </div>
            }
        })
        .collect::<Vec<_>>();

    view! {
        <Show when=open>
            <div class="help-overlay"
                 on:click=move |_| state.update(|s| s.help_open = false)>
                <div class="help-modal" role="dialog" aria-modal="true" aria-label="Keyboard shortcuts"
                     on:click=|e| e.stop_propagation()>
                    <div class="help-head">
                        <h2>"Keyboard shortcuts"</h2>
                        <button type="button" class="help-close" aria-label="Close"
                                on:click=move |_| state.update(|s| s.help_open = false)>"×"</button>
                    </div>
                    <div class="help-body">{sections.clone()}</div>
                </div>
            </div>
        </Show>
    }
}
