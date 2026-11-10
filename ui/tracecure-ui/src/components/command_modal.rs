//! `:` command palette modal. Mirrors the TUI's command prompt, plus a
//! list of the verbs that match what has been typed — the TUI can assume
//! its user knows them; a web page cannot.

use leptos::prelude::*;
use tracecure_ui::command::{complete, parse, suggest};
use tracecure_ui::state::AppState;

use crate::app::AppCtx;

#[component]
pub fn CommandModal() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;
    let open = move || state.with(|s| s.command_open);
    let value = Memo::new(move |_| state.with(|s| s.command_input.clone()));

    let input_ref = NodeRef::<leptos::html::Input>::new();
    // `autofocus` only applies at page load, so focus by hand when the
    // palette mounts.
    Effect::new(move |_| {
        if let Some(input) = input_ref.get() {
            let _ = input.focus();
        }
    });

    let on_input = move |e: leptos::ev::Event| {
        let v = event_target_value(&e);
        state.update(|s| s.command_input = v);
    };

    let on_keydown = move |e: leptos::ev::KeyboardEvent| {
        let key = e.key();
        match key.as_str() {
            "Escape" => {
                e.prevent_default();
                // The global handler closes the palette too; stop here so
                // one Escape does not also collapse an open detail pane.
                e.stop_propagation();
                state.update(AppState::close_command_prompt);
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

    let suggestions = move || {
        suggest(&value.get())
            .into_iter()
            .map(|h| {
                // Clicking a verb fills it in; a verb with no arguments is
                // then one Enter away, and one with arguments is ready for
                // them.
                let fill = if h.usage == h.verb {
                    h.verb.to_string()
                } else {
                    format!("{} ", h.verb)
                };
                view! {
                    <button type="button" class="cmd-suggestion"
                            on:click=move |_| {
                                state.update(|s| s.command_input.clone_from(&fill));
                                if let Some(input) = input_ref.get_untracked() {
                                    let _ = input.focus();
                                }
                            }>
                        <span class="cmd-usage">{format!(":{}", h.usage)}</span>
                        <span class="cmd-about">{h.about}</span>
                    </button>
                }
            })
            .collect_view()
    };

    view! {
        <Show when=open>
            <div class="cmd-overlay"
                 on:click=move |_| state.update(AppState::close_command_prompt)>
                <div class="cmd-modal" role="dialog" aria-modal="true" aria-label="Command palette"
                     on:click=|e| e.stop_propagation()>
                    <div class="cmd-suggestions">{suggestions}</div>
                    <div class="cmd-line">
                        <span class="cmd-prefix" aria-hidden="true">":"</span>
                        <input type="text"
                               class="cmd-input"
                               aria-label="Command"
                               placeholder="type a command — Tab completes, Enter runs"
                               spellcheck="false"
                               autocomplete="off"
                               autocapitalize="off"
                               node_ref=input_ref
                               prop:value=move || value.get()
                               on:input=on_input
                               on:keydown=on_keydown/>
                    </div>
                </div>
            </div>
        </Show>
    }
}
