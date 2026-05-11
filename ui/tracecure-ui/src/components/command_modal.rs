//! `:` command palette modal. Mirrors the TUI's command prompt.

use leptos::prelude::*;
use tracecure_ui::command::{complete, parse};

use crate::app::AppCtx;

#[component]
pub fn CommandModal() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;
    let open = move || state.with(|s| s.command_open);
    let value = move || state.with(|s| s.command_input.clone());

    let on_input = move |e: leptos::ev::Event| {
        let v = event_target_value(&e);
        state.update(|s| s.command_input = v);
    };

    let on_keydown = move |e: leptos::ev::KeyboardEvent| {
        let key = e.key();
        match key.as_str() {
            "Escape" => {
                e.prevent_default();
                state.update(tracecure_ui::state::AppState::close_command_prompt);
            }
            "Enter" => {
                e.prevent_default();
                let input = state.with(|s| s.command_input.clone());
                match parse(&input) {
                    Ok(action) => {
                        state.update(|s| {
                            s.pending_command = Some(action);
                            s.close_command_prompt();
                        });
                    }
                    Err(err) => {
                        state.update(|s| s.set_flash_err(format!("bad command: {err}")));
                    }
                }
            }
            "Tab" => {
                e.prevent_default();
                let input = state.with(|s| s.command_input.clone());
                let c = complete(&input);
                if !c.prefix.is_empty() {
                    state.update(|s| s.command_input = c.prefix);
                }
            }
            _ => {}
        }
    };

    view! {
        <Show when=open>
            <div class="cmd-overlay"
                 on:click=move |_| state.update(tracecure_ui::state::AppState::close_command_prompt)>
                <div class="cmd-modal" on:click=|e| e.stop_propagation()>
                    <span class="cmd-prefix">":"</span>
                    <input type="text"
                           class="cmd-input"
                           prop:value=value
                           on:input=on_input
                           on:keydown=on_keydown
                           autofocus="autofocus"/>
                </div>
            </div>
        </Show>
    }
}
