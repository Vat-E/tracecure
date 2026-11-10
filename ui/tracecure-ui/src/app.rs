//! App shell — owns the `AppState` signal, builds the layout.

use leptos::ev::KeyboardEvent;
use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use tracecure_ui::state::{
    AppState, FetchAnchor, LevelSet, Selection, TimeWindow, ViewOpts, RECORD_CAP,
};
use tracecure_ui::urlstate::UrlState;

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

/// How many records an explicit fetch asks for: a search, a time-range
/// change. Bigger than the mount backfill because the user asked for it and
/// is waiting on the answer, not on first paint.
pub const WINDOW_FETCH_LIMIT: usize = 2000;

/// The most a fetch can usefully ask for — the buffer holds no more.
pub const LOAD_ALL_LIMIT: usize = RECORD_CAP;

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

/// The viewer's UTC offset in seconds, read once at mount. Timestamps are
/// stored and sent as UTC; this is only for display.
#[derive(Clone, Copy)]
pub struct TzCtx(pub i32);

/// The free-text term rows highlight. Its own signal because every mounted
/// row reads it: it must change only when the term does, not on every state
/// write.
#[derive(Clone, Copy)]
pub struct HighlightCtx(pub RwSignal<String>);

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

    provide_context(TzCtx(local_utc_offset_secs()));

    let highlight = RwSignal::new(String::new());
    provide_context(HighlightCtx(highlight));
    Effect::new(move |_| {
        let term = state.with(|s| s.filter().text.clone());
        if highlight.with_untracked(|h| *h != term) {
            highlight.set(term);
        }
    });

    // Legacy URL migration (best-effort): if the loaded URL is
    // /tail or /search, parse params into state and rewrite to /.
    migrate_legacy_url(state);

    // Restore the view the URL describes — before the backfill, so the first
    // fetch already asks for the right window and text.
    let initial_search = restore_from_url(state);

    // Kick off WS connection, keeping the end of the channel that lets
    // `:retry` and a server-side lag error force a reconnect.
    let (reconnect_tx, reconnect_rx) = futures::channel::mpsc::unbounded::<()>();
    provide_context(ReconnectCtx(reconnect_tx.clone()));
    crate::ws::connect_forever(AppCtx(state), reconnect_rx);

    // Backfill history at mount. `ws` also calls this after a reconnect.
    spawn_backfill(state);

    // Keep the URL describing the current view, so it can be bookmarked or
    // shared. `replaceState`, not `pushState`: typing a query should not
    // bury the previous page under one history entry per keystroke.
    let last_search = StoredValue::new(initial_search);
    Effect::new(move |_| {
        let search = state.with(|s| url_state_of(s).to_search());
        if last_search.with_value(|l| *l != search) {
            if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
                let _ = history.replace_state_with_url(
                    &wasm_bindgen::JsValue::NULL,
                    "",
                    Some(&format!("/{search}")),
                );
            }
            last_search.set_value(search);
        }
    });

    // Global keyboard listener.
    let state_for_keys = state;
    let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        handle_global_key(state_for_keys, selection, view_opts, &reconnect_tx, &e);
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
    Effect::new(move |prev: Option<TimeWindow>| {
        let current = state.with(|s| s.time_window.clone());

        // Skip first run (no prior value).
        let Some(p) = prev.as_ref() else {
            return current;
        };

        // No-op if unchanged (this Effect would otherwise fire on any signal touch).
        if *p == current {
            return current;
        }

        run_window_fetch(state, WINDOW_FETCH_LIMIT, FetchAnchor::Newest);

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
        // Top up with what the buffer already holds, not with whatever is
        // being typed: a reconnect mid-edit must not mix a half-typed search
        // into a buffer loaded for a different one.
        let text = state.with_untracked(|s| {
            if s.loaded.limit == 0 {
                s.filter().text.clone()
            } else {
                s.loaded.text.clone()
            }
        });
        let (from, to) = window_bounds(&window);
        match tracecure_ui::query::fetch_window(&text, from, to, BACKFILL_LIMIT).await {
            Ok(records) => state.update(|s| {
                // Only the first load describes the buffer. A later backfill
                // (reconnect, lag recovery) tops up a buffer that an explicit
                // fetch may have loaded more deeply; recording its smaller
                // limit would claim a truncation that is not there.
                if s.loaded.limit == 0 {
                    s.note_backfill(text, BACKFILL_LIMIT, records.len());
                }
                s.merge_backfill(records);
            }),
            Err(e) => state.update(|s| s.set_flash_err(format!("backfill failed: {e}"))),
        }
    });
}

/// Replace the buffer with the current window, matched on the query's free
/// text by the server.
///
/// This is the one path every explicit load takes — Enter in the query box,
/// `:search`, a time-range change, "load more" — so they cannot drift apart
/// on what they ask for or how they report it. Responses are generation
/// checked: a slow reply to an earlier request never overwrites a later one.
pub fn run_window_fetch(state: RwSignal<AppState>, limit: usize, anchor: FetchAnchor) {
    spawn_local(async move {
        let mut gen = 0;
        state.update(|s| gen = s.begin_fetch());
        let (window, text) =
            state.with_untracked(|s| (s.time_window.clone(), s.filter().text.clone()));
        let (from, to) = window_bounds(&window);
        match tracecure_ui::query::fetch_window(&text, from, to, limit).await {
            Ok(records) => state.update(|s| {
                s.finish_fetch(gen, text, limit, records, anchor);
            }),
            Err(e) => state.update(|s| s.fail_fetch(gen, format!("search failed: {e}"))),
        }
    });
}

#[component]
fn Home() -> impl IntoView {
    view! {
        // Not `.app`: the rows have an `.app` cell, and a shared class name
        // gave every one of them this element's width and flex layout.
        <div class="shell">
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
    reconnect: &futures::channel::mpsc::UnboundedSender<()>,
    e: &KeyboardEvent,
) {
    use tracecure_core::Level;

    // Letter bindings are case-significant (`g` top, `G` latest; `t`
    // timestamps, `T` trace). Take the case from the Shift state rather than
    // trusting `key` alone: some input paths report the unshifted letter
    // with `shiftKey` set, which would send Shift+G to the top of the list.
    let key = {
        let k = e.key();
        if e.shift_key() && k.len() == 1 && k.chars().all(|c| c.is_ascii_lowercase()) {
            k.to_ascii_uppercase()
        } else {
            k
        }
    };
    let active = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element());
    let active_tag = active.as_ref().map(web_sys::Element::tag_name);
    // Don't swallow keys when the user is typing.
    let in_input = matches!(active_tag.as_deref(), Some("INPUT" | "TEXTAREA"));

    if key == "Escape" {
        e.prevent_default();
        // Innermost thing first: palette, help, the query box, then the
        // detail pane. Each arm is gated so Escape on an idle UI writes
        // nothing.
        let (cmd_open, help_open) = state.with_untracked(|s| (s.command_open, s.help_open));
        if cmd_open {
            state.update(AppState::close_command_prompt);
        } else if help_open {
            state.update(|s| s.help_open = false);
        } else if in_input {
            if let Some(el) = active.and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok()) {
                let _ = el.blur();
            }
        } else if selection.with_untracked(|s| s.detail_expanded) {
            selection.update(Selection::close);
        }
        return;
    }

    // Chords belong to the browser and the OS. (Ctrl+T used to be bound
    // here; the browser opens a tab before the page ever sees it.)
    if in_input || e.ctrl_key() || e.meta_key() || e.alt_key() {
        return;
    }
    // While help is up, only its own toggle is live.
    if state.with_untracked(|s| s.help_open) && key != "?" {
        return;
    }
    // A focused button handles its own Space/Enter; acting on them here too
    // would fire two actions for one keypress.
    if active_tag.as_deref() == Some("BUTTON") && matches!(key.as_str(), " " | "Enter") {
        return;
    }

    let toggle_level = |lvl: Level| state.update(|s| s.toggle_level(lvl));

    match key.as_str() {
        "?" => state.update(AppState::toggle_help),
        ":" => state.update(AppState::open_command_prompt),
        "/" => focus_query_input(),
        "t" => view_opts.update(ViewOpts::toggle_timestamp_mode),
        "G" | "End" => state.update(AppState::resume_live),
        "g" | "Home" => {
            if state.with_untracked(|s| s.live) {
                state.update(AppState::pause);
            }
            if let Some(el) = stream_element() {
                el.set_scroll_top(0);
            }
        }
        " " => state.update(|s| {
            if s.live {
                s.pause();
            } else {
                s.resume_live();
            }
        }),
        "j" | "ArrowDown" => move_selection(state, selection, 1),
        "k" | "ArrowUp" => move_selection(state, selection, -1),
        "Enter" => {
            let Some(seq) = selection.with_untracked(|s| s.selected_seq) else {
                return;
            };
            if state.with_untracked(|s| s.live) {
                state.update(AppState::pause);
            }
            selection.update(|s| s.toggle(seq));
            scroll_selected_into_view();
        }
        "r" => {
            let _ = reconnect.unbounded_send(());
            state.update(|s| s.set_flash_ok("reconnecting…"));
        }
        "I" => toggle_level(Level::Info),
        "W" => toggle_level(Level::Warn),
        "E" => toggle_level(Level::Error),
        "F" => toggle_level(Level::Fatal),
        "D" => toggle_level(Level::Debug),
        "T" => toggle_level(Level::Trace),
        _ => return,
    }
    e.prevent_default();
}

/// Step the selection through the visible rows.
///
/// Pauses the tail first: a row cannot be read while the list is moving
/// under it, and with new records held in `pending` the selected row stays
/// where the user put it.
fn move_selection(state: RwSignal<AppState>, selection: RwSignal<Selection>, delta: isize) {
    let current = selection.with_untracked(|s| s.selected_seq);
    let Some(next) = state.with_untracked(|s| s.neighbor_seq(current, delta)) else {
        return;
    };
    if state.with_untracked(|s| s.live) {
        state.update(AppState::pause);
    }
    selection.update(|s| s.select(next));
    scroll_selected_into_view();
}

fn stream_element() -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector(".stream").ok().flatten())
}

/// Bring the selected row into view once the DOM reflects the new selection.
fn scroll_selected_into_view() {
    spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(0).await;
        let Some(row) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.query_selector(".stream .row.selected").ok().flatten())
        else {
            return;
        };
        let opts = web_sys::ScrollIntoViewOptions::new();
        opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
        row.scroll_into_view_with_scroll_into_view_options(&opts);
    });
}

fn focus_query_input() {
    if let Some(input) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector(".query input").ok().flatten())
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = input.focus();
    }
}

fn dispatch_command(state: RwSignal<AppState>, action: tracecure_ui::command::CommandAction) {
    use tracecure_ui::command::CommandAction as A;
    match action {
        A::Pause => state.update(|s| {
            s.pause();
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
            // The palette and the query box are two ways into one search:
            // `q` becomes the box's text, so what was asked for stays visible
            // and editable instead of living in state nobody can see.
            if let Some(q) = q {
                state.update(|s| s.set_query(q));
            }
            let window_changes = match (from, to) {
                (Some(from), Some(to)) => {
                    let custom = TimeWindow::Custom { from, to };
                    let changes = state.with_untracked(|s| s.time_window != custom);
                    if changes {
                        state.update(|s| s.time_window = custom);
                    }
                    changes
                }
                _ => false,
            };
            // A window change refetches through its Effect; otherwise fetch
            // here, or an unchanged window would mean no search at all.
            if !window_changes {
                run_window_fetch(state, WINDOW_FETCH_LIMIT, FetchAnchor::Newest);
            }
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

fn window_bounds(w: &TimeWindow) -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
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
            state.update(|s| s.set_query(q));
        }
        // Replace URL to / without reloading.
        if let Ok(history) = w.history() {
            let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some("/"));
        }
    }
}

/// The part of the view worth putting in the URL. A fixed (`Custom`) window
/// is left out: it comes from `:search from= to=`, and re-opening the link
/// later should not silently pin the view to a range in the past.
fn url_state_of(s: &AppState) -> UrlState {
    UrlState {
        query: s.query().to_string(),
        window_secs: match &s.time_window {
            TimeWindow::Preset(d) if *d != std::time::Duration::from_secs(3600) => {
                Some(d.as_secs())
            }
            _ => None,
        },
        levels: (s.level_set != LevelSet::defaults()).then_some(s.level_set),
    }
}

/// Apply `location.search` to the state. Returns the canonical search
/// string for what was applied, so the URL-sync effect starts in step.
fn restore_from_url(state: RwSignal<AppState>) -> String {
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    let url = UrlState::parse(&search);
    state.update(|s| {
        if !url.query.is_empty() {
            s.set_query(url.query.clone());
        }
        if let Some(secs) = url.window_secs {
            s.time_window = TimeWindow::Preset(std::time::Duration::from_secs(secs));
        }
        if let Some(levels) = url.levels {
            s.level_set = levels;
        }
    });
    state.with_untracked(|s| url_state_of(s).to_search())
}

/// The browser's current UTC offset, in seconds east of UTC.
fn local_utc_offset_secs() -> i32 {
    // `getTimezoneOffset` is minutes *behind* UTC, hence the sign flip.
    #[allow(clippy::cast_possible_truncation)]
    let behind_mins = js_sys::Date::new_0().get_timezone_offset() as i32;
    -behind_mins * 60
}
