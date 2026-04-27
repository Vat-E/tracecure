//! App shell — owns the `AppState` signal, builds the layout.

use leptos::ev::KeyboardEvent;
use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use sift_ui::state::AppState;
use sift_ui::state::Status;

use crate::components::command_modal::CommandModal;
use crate::components::flash::Flash;
use crate::components::help_modal::HelpModal;
use crate::components::paused_banner::PausedBanner;
use crate::components::rail::Rail;
use crate::components::search_row::SearchRow;
use crate::components::status_bar::StatusBar;
use crate::components::stream::Stream;
use crate::components::top_bar::TopBar;

/// Global state context — every component reads via `expect_context`.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct AppCtx(pub RwSignal<AppState>);

#[component]
pub fn App() -> impl IntoView {
    // Shared state signal.
    let state = RwSignal::new(AppState::new());
    provide_context(AppCtx(state));

    // Kick off WS connection.
    crate::ws::connect_forever(AppCtx(state));

    // Legacy URL migration (best-effort): if the loaded URL is
    // /tail or /search, parse params into state and rewrite to /.
    migrate_legacy_url(state);

    // Global keyboard listener.
    let state_for_keys = state;
    let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        handle_global_key(state_for_keys, &e);
    });
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        let _ = doc.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
        cb.forget();
    }

    // Dispatch pending commands.
    Effect::new(move |_| {
        if let Some(action) = state.with(|s| s.pending_command.clone()) {
            dispatch_command(state, action);
            state.update(|s| s.pending_command = None);
        }
    });

    view! {
        <Router>
            <Routes fallback=|| view! { <Home/> }>
                <Route path=path!("/") view=Home/>
            </Routes>
        </Router>
    }
}

#[component]
fn Home() -> impl IntoView {
    view! {
        <div class="app">
            <Rail/>
            <main class="main">
                <TopBar/>
                <Stream/>
                <SearchRow/>
                <StatusBar/>
            </main>
            <PausedBanner/>
            <HelpModal/>
            <CommandModal/>
            <Flash/>
        </div>
    }
}

fn handle_global_key(state: RwSignal<AppState>, e: &KeyboardEvent) {
    let key = e.key();
    let ctrl = e.ctrl_key();

    // Don't swallow keys when user is typing in an <input>.
    let in_input = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .is_some_and(|el| el.tag_name() == "INPUT" || el.tag_name() == "TEXTAREA");

    match (key.as_str(), ctrl, in_input) {
        ("Escape", _, _) => {
            e.prevent_default();
            state.update(|s| {
                if s.command_open {
                    s.close_command_prompt();
                } else if s.help_open {
                    s.help_open = false;
                } else if s.detail_expanded {
                    s.close_detail();
                }
            });
        }
        ("?", false, false) => {
            e.prevent_default();
            state.update(AppState::toggle_help);
        }
        (":", false, false) => {
            e.prevent_default();
            state.update(AppState::open_command_prompt);
        }
        ("/", false, false) => {
            e.prevent_default();
            if let Some(input) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector(".query input").ok().flatten())
                .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let _ = input.focus();
            }
        }
        ("t", true, _) => {
            e.prevent_default();
            state.update(AppState::toggle_timestamp_mode);
        }
        ("G" | "End", false, false) => {
            e.prevent_default();
            state.update(AppState::resume_live);
        }
        (" ", false, false) => {
            e.prevent_default();
            state.update(|s| {
                if s.live {
                    s.live = false;
                } else {
                    s.resume_live();
                }
            });
        }
        ("I", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Info)),
        ("W", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Warn)),
        ("E", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Error)),
        ("F", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Fatal)),
        ("D", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Debug)),
        ("T", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Trace)),
        _ => {}
    }
}

fn dispatch_command(state: RwSignal<AppState>, action: sift_ui::command::CommandAction) {
    use sift_ui::command::CommandAction as A;
    match action {
        A::Pause => state.update(|s| {
            s.live = false;
            s.set_flash_ok("paused");
        }),
        A::Resume => state.update(|s| {
            s.resume_live();
            s.set_flash_ok("resumed");
        }),
        A::Clear => state.update(AppState::clear_records),
        A::Retry => {
            // Kick reconnect by toggling status — actual reconnect handled by ws loop.
            state.update(|s| {
                s.status = Status::Disconnected;
                s.set_flash_ok("retry triggered");
            });
        }
        A::Save { path } => {
            // Serialize records → NDJSON → browser download via synthetic <a download>.
            let records: Vec<sift_core::Record> =
                state.with(|s| s.records.iter().cloned().collect());
            let mut body = String::new();
            for r in &records {
                if let Ok(line) = serde_json::to_string(r) {
                    body.push_str(&line);
                    body.push('\n');
                }
            }
            let filename = path.file_name().map_or_else(
                || "sift-records.ndjson".to_string(),
                |os| os.to_string_lossy().into_owned(),
            );
            if let Err(e) = trigger_download(&body, &filename) {
                state.update(|s| s.set_flash_err(format!("save failed: {e}")));
            } else {
                state.update(|s| {
                    s.set_flash_ok(format!("saved {} records to {}", records.len(), filename));
                });
            }
        }
        A::Search { q, from: _, to: _ } => {
            state.update(|s| {
                if let Some(q) = q {
                    s.query = q;
                }
                s.set_flash_ok("search updated");
            });
        }
        A::Level { level, on } => {
            state.update(|s| {
                let current = s.level_set.allows(level);
                if current != on {
                    s.toggle_level(level);
                }
                s.set_flash_ok(format!("{level:?} {}", if on { "on" } else { "off" }));
            });
        }
        A::Help => state.update(|s| s.help_open = true),
        A::Quit => {
            if let Some(w) = web_sys::window() {
                let _ = w.close();
            }
        }
    }
}

/// Trigger a browser download of `body` as a file named `filename`.
fn trigger_download(body: &str, filename: &str) -> Result<(), String> {
    let blob_parts = js_sys::Array::new();
    blob_parts.push(&wasm_bindgen::JsValue::from_str(body));
    let blob_opts = web_sys::BlobPropertyBag::new();
    blob_opts.set_type("application/x-ndjson");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&blob_parts, &blob_opts)
        .map_err(|_| "blob create failed".to_string())?;

    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "object url failed".to_string())?;

    let doc = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| "no document".to_string())?;
    let a = doc
        .create_element("a")
        .map_err(|_| "a create failed".to_string())?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "not an anchor".to_string())?;
    a.set_href(&url);
    a.set_download(filename);
    a.click();

    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}

fn migrate_legacy_url(state: RwSignal<AppState>) {
    use web_sys::window;
    let Some(w) = window() else { return };
    let Ok(loc) = w.location().href() else { return };
    let Ok(url) = web_sys::Url::new(&loc) else {
        return;
    };

    let path = url.pathname();
    if path == "/tail" || path == "/search" {
        // Extract ?q=... and populate state.query if present.
        let params = url.search_params();
        if let Some(q) = params.get("q") {
            state.update(|s| s.query = q);
        }
        // Replace URL to / without reloading.
        if let Ok(history) = w.history() {
            let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some("/"));
        }
    }
}
