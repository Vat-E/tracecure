//! App shell — owns the `AppState` signal, builds the layout.

use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

use sift_ui::state::AppState;

use crate::components::command_modal::CommandModal;
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

    // Legacy URL migration (best-effort): if the loaded URL is
    // /tail or /search, parse params into state and rewrite to /.
    migrate_legacy_url(state);

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
        </div>
    }
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
