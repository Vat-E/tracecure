//! App shell — owns the `AppState` signal, builds the layout.

use leptos::ev::KeyboardEvent;
use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use tracecure_ui::state::AppState;

use tracecure_ui::state::{Selection, ViewOpts};

use crate::components::command_modal::CommandModal;
use crate::components::flash::Flash;
use crate::components::help_modal::HelpModal;
use crate::components::paused_banner::PausedBanner;
use crate::components::rail::Rail;
use crate::components::rate_histogram::RateHistogram;
use crate::components::search_row::SearchRow;
use crate::components::status_bar::StatusBar;
use crate::components::stream::Stream;
use crate::components::top_bar::TopBar;

/// How many historical records to pull at mount. Enough to fill the viewport
/// deeply without paying for a multi-MB JSON parse on the main thread.
const BACKFILL_LIMIT: usize = 500;

/// Global state context — every component reads via `expect_context`.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct AppCtx(pub RwSignal<AppState>);

/// Selection lives in its own signal, separate from [`AppCtx`].
///
/// Every mounted row subscribes to it. Leptos's `RwSignal::update` notifies
/// all subscribers unconditionally — there is no field-level diffing — so
/// while selection sat inside `AppState`, one arriving log record woke a
/// closure in each of up to 5000 rows. Splitting the signal is what makes an
/// ingested record cost nothing for rows that did not change.
#[derive(Clone, Copy)]
pub struct SelectionCtx(pub RwSignal<Selection>);

/// Per-row presentation options. Split from [`AppCtx`] for the same reason
/// as [`SelectionCtx`].
#[derive(Clone, Copy)]
pub struct ViewCtx(pub RwSignal<ViewOpts>);

/// A 1 Hz tick for anything whose rendering depends on the wall clock
/// rather than on state: relative timestamps, and the rolling time window.
///
/// These used to refresh as an accidental side effect of the flash ticker
/// writing state twice a second whether or not there was a flash. That write
/// is now gated, so the dependency has to be explicit. Consumers read this
/// *only* on the paths that actually need a clock — Leptos re-collects
/// dependencies on every run, so a row rendering an absolute timestamp never
/// subscribes to it.
#[derive(Clone, Copy)]
pub struct ClockCtx(pub RwSignal<u64>);

/// Lets any component ask the WebSocket loop to reconnect now.
#[derive(Clone)]
pub struct ReconnectCtx(pub futures::channel::mpsc::UnboundedSender<()>);

#[component]
pub fn App() -> impl IntoView {
    // Shared state signal.
    let state = RwSignal::new(AppState::new());
    provide_context(AppCtx(state));

    let selection = RwSignal::new(Selection::default());
    provide_context(SelectionCtx(selection));
    let view_opts = RwSignal::new(ViewOpts::default());
    provide_context(ViewCtx(view_opts));

    // Wall-clock tick. Cheap: one signal write a second, and only the
    // components that actually read a clock re-run.
    let clock = RwSignal::new(0u64);
    provide_context(ClockCtx(clock));
    spawn_local(async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(1000).await;
            clock.update(|t| *t = t.wrapping_add(1));
        }
    });

    // Kick off WS connection, keeping the end of the channel that lets
    // `:retry` and a server-side lag error force a reconnect.
    let (reconnect_tx, reconnect_rx) = futures::channel::mpsc::unbounded::<()>();
    provide_context(ReconnectCtx(reconnect_tx));
    crate::ws::connect_forever(AppCtx(state), reconnect_rx);

    // Backfill history at mount. `ws` also calls this after a reconnect.
    spawn_backfill(state);

    // Legacy URL migration (best-effort): if the loaded URL is
    // /tail or /search, parse params into state and rewrite to /.
    migrate_legacy_url(state);

    // Global keyboard listener.
    let state_for_keys = state;
    let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        handle_global_key(state_for_keys, selection, view_opts, &e);
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

    // Effect: when time_window changes, refetch from backend.
    Effect::new(move |prev: Option<tracecure_ui::state::TimeWindow>| {
        let current = state.with(|s| s.time_window.clone());

        // Skip first run (no prior value).
        let Some(p) = prev.as_ref() else {
            return current;
        };

        // No-op if unchanged (this Effect would otherwise fire on any signal touch).
        if *p == current {
            return current;
        }

        // Fire backend query asynchronously.
        let current_snapshot = current.clone();
        spawn_local(async move {
            let (from, to) = window_bounds(&current_snapshot);
            let q = state.with(|s| s.query.clone());
            match tracecure_ui::query::fetch_window(&q, from, to, 500).await {
                Ok(records) => {
                    state.update(|s| s.replace_records(records));
                }
                Err(e) => {
                    state.update(|s| s.set_flash_err(format!("query failed: {e:?}")));
                }
            }
        });

        current
    });

    view! {
        <Router>
            <Routes fallback=|| view! { <Home/> }>
                <Route path=path!("/") view=Home/>
            </Routes>
        </Router>
    }
}

/// Fetch the current window's history and merge it into the buffer.
///
/// Needed because `/v1/tail` is a bare `broadcast::subscribe()` server-side:
/// it only ever delivers records ingested *after* the socket opens, and has
/// no replay. So the buffer starts empty on load, and any records ingested
/// while a dropped socket was reconnecting exist in neither the buffer nor
/// the new stream. Both gaps are closed by re-running this.
///
/// Safe to call repeatedly and concurrently with live delivery:
/// `merge_backfill` dedups against what is already held and re-sorts by
/// timestamp. Reads are untracked so this never registers as a subscriber.
pub fn spawn_backfill(state: RwSignal<AppState>) {
    spawn_local(async move {
        let window = state.with_untracked(|s| s.time_window.clone());
        let q = state.with_untracked(|s| s.query.clone());
        let (from, to) = window_bounds(&window);
        match tracecure_ui::query::fetch_window(&q, from, to, BACKFILL_LIMIT).await {
            Ok(records) => state.update(|s| s.merge_backfill(records)),
            Err(e) => state.update(|s| s.set_flash_err(format!("backfill failed: {e}"))),
        }
    });
}

#[component]
fn Home() -> impl IntoView {
    view! {
        <div class="app">
            <Rail/>
            <main class="main">
                <TopBar/>
                <RateHistogram/>
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

fn handle_global_key(
    state: RwSignal<AppState>,
    selection: RwSignal<Selection>,
    view_opts: RwSignal<ViewOpts>,
    e: &KeyboardEvent,
) {
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
            // Same precedence as before (command > help > detail), but each
            // arm is gated so Escape on an already-closed UI writes nothing.
            let (cmd_open, help_open) = state.with_untracked(|s| (s.command_open, s.help_open));
            if cmd_open {
                state.update(AppState::close_command_prompt);
            } else if help_open {
                state.update(|s| s.help_open = false);
            } else if selection.with_untracked(|s| s.detail_expanded) {
                selection.update(Selection::close);
            }
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
            view_opts.update(ViewOpts::toggle_timestamp_mode);
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
        ("I", false, false) => state.update(|s| s.toggle_level(tracecure_core::Level::Info)),
        ("W", false, false) => state.update(|s| s.toggle_level(tracecure_core::Level::Warn)),
        ("E", false, false) => state.update(|s| s.toggle_level(tracecure_core::Level::Error)),
        ("F", false, false) => state.update(|s| s.toggle_level(tracecure_core::Level::Fatal)),
        ("D", false, false) => state.update(|s| s.toggle_level(tracecure_core::Level::Debug)),
        ("T", false, false) => state.update(|s| s.toggle_level(tracecure_core::Level::Trace)),
        _ => {}
    }
}

fn dispatch_command(state: RwSignal<AppState>, action: tracecure_ui::command::CommandAction) {
    use tracecure_ui::command::CommandAction as A;
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
            // Previously this only wrote `Status::Disconnected` and claimed
            // in a comment that the ws loop would act on it. It does not —
            // the loop parks on the socket's close channel and never reads
            // status back. Signal it explicitly.
            match use_context::<ReconnectCtx>() {
                Some(ReconnectCtx(tx)) => {
                    let _ = tx.unbounded_send(());
                    state.update(|s| s.set_flash_ok("reconnecting…"));
                }
                None => state.update(|s| s.set_flash_err("reconnect unavailable")),
            }
        }
        A::Save { path } => {
            // Serialize records → NDJSON → browser download via synthetic <a download>.
            let records: Vec<tracecure_core::Record> =
                state.with(|s| s.records.iter().map(|e| (*e.rec).clone()).collect());
            let mut body = String::new();
            for r in &records {
                if let Ok(line) = serde_json::to_string(r) {
                    body.push_str(&line);
                    body.push('\n');
                }
            }
            let filename = path.file_name().map_or_else(
                || "tracecure-records.ndjson".to_string(),
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
        A::Search { q, from, to } => {
            // Update query immediately (client-side filter).
            if let Some(ref qval) = q {
                state.update(|s| s.query.clone_from(qval));
            }
            // If from/to provided, switch to Custom window (this triggers
            // the Effect refetch). Otherwise fire fetch_window directly
            // against the current rolling window.
            let q_for_fetch = q.unwrap_or_default();
            if let (Some(from), Some(to)) = (from, to) {
                state.update(|s| {
                    s.time_window = tracecure_ui::state::TimeWindow::Custom { from, to }
                });
                // The time_window Effect (T24) handles the fetch.
            } else {
                // Fire fetch directly with current window bounds.
                let current_window = state.with(|s| s.time_window.clone());
                spawn_local(async move {
                    let (from, to) = window_bounds(&current_window);
                    match tracecure_ui::query::fetch_window(&q_for_fetch, from, to, 500).await {
                        Ok(records) => state.update(|s| s.replace_records(records)),
                        Err(e) => state.update(|s| s.set_flash_err(format!("query failed: {e:?}"))),
                    }
                });
            }
            state.update(|s| s.set_flash_ok("search submitted"));
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

fn window_bounds(
    w: &tracecure_ui::state::TimeWindow,
) -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
    use tracecure_ui::state::TimeWindow;
    match w {
        TimeWindow::Preset(d) => {
            let to = chrono::Utc::now();
            let from =
                to - chrono::Duration::from_std(*d).unwrap_or_else(|_| chrono::Duration::hours(1));
            (from, to)
        }
        TimeWindow::Custom { from, to } => (*from, *to),
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
