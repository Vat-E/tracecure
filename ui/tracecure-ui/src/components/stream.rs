//! Stream container — scrollable list of rows, with the notice for a
//! truncated buffer above them and the empty state in their place.
//!
//! ## How the tail is followed
//!
//! Three mechanisms, each covering what the others cannot:
//!
//! 1. **CSS scroll anchoring.** `.stream-anchor` is the only element in the
//!    container the browser may anchor to (see `styles.css`). While it is in
//!    view — i.e. the user is at the bottom — rows inserted above it push it
//!    down and the browser scrolls to keep it in place. That is the browser's
//!    own layout pass, so it costs no forced reflow from script, which is
//!    what froze the renderer when this was done with `scrollTop` per batch.
//! 2. **A 1 Hz re-pin.** `.row` uses `content-visibility`, so heights below
//!    the fold are estimates and anchoring can land a little short; Safari
//!    has no scroll anchoring at all. Once a second, while live, snap to the
//!    bottom if we have drifted. One layout read per second is harmless.
//! 3. **Explicit scroll requests** (`scroll_request_counter`) for resume and
//!    for a freshly loaded buffer.
//!
//! ## How pausing is decided
//!
//! By user intent, never by scroll position alone. A scroll event is acted
//! on only if it follows a wheel, touch, scrollbar press or scroll key. The
//! old rule — "not at the bottom means the user scrolled up" — also fired
//! for every scroll the *layout* caused: a window resize, a filter changing
//! the list height, a refetch. The tail paused and resumed by itself.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use tracecure_ui::state::{AppState, Entry, FetchAnchor, RECORD_CAP};

use crate::app::{run_window_fetch, AppCtx, ClockCtx, LOAD_ALL_LIMIT};
use crate::components::row::Row;

// Px from the bottom that still counts as "at bottom". Generous because
// `.row` uses `content-visibility`, so `scroll_height` is partly an estimate
// from `contain-intrinsic-size` and lands a row or two off the true bottom.
// A tight threshold reads that estimation error as the user scrolling up.
const SCROLL_BOTTOM_THRESHOLD: f64 = 48.0;

/// How long after a wheel / touch / key a scroll event still counts as the
/// user's. Each user-driven scroll event renews it, so a long drag or a
/// momentum scroll stays covered.
const INTENT_WINDOW_MS: f64 = 300.0;
/// Pressing the scrollbar produces no further input events until the drag
/// moves, so it gets a longer head start.
const SCROLLBAR_PRESS_MS: f64 = 1500.0;

fn gap_to_bottom(el: &web_sys::HtmlDivElement) -> f64 {
    f64::from(el.scroll_height()) - f64::from(el.scroll_top()) - f64::from(el.client_height())
}

#[component]
pub fn Stream() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;
    let clock = expect_context::<ClockCtx>().0;

    let stream_ref = NodeRef::<leptos::html::Div>::new();

    // `Date.now()` until which a scroll event is attributed to the user.
    let intent_until = Rc::new(Cell::new(0.0_f64));
    let mark_intent = {
        let intent_until = intent_until.clone();
        move |ms: f64| intent_until.set(js_sys::Date::now() + ms)
    };

    // Reactive iteration over filtered records. `filtered()` hands back
    // `Entry`s, which share their `Record` via `Arc` — no deep copy here.
    let rows = move || state.with(AppState::filtered);

    // DOM scroll on counter bump. First run captures the current counter
    // without scrolling; subsequent runs scroll to the bottom whenever the
    // counter changes (resume, a new buffer).
    //
    // Deferred so the `For` reconciler has patched the new rows in before
    // `scroll_height` is read, and repeated once: the first pass renders the
    // rows that were below the fold, which replaces their estimated heights
    // with real ones and moves the true bottom.
    Effect::new(move |last: Option<u64>| -> u64 {
        let current = state.with(|s| s.scroll_request_counter);
        if last.is_some_and(|prev| prev != current) {
            wasm_bindgen_futures::spawn_local(async move {
                for delay_ms in [0, 60] {
                    gloo_timers::future::TimeoutFuture::new(delay_ms).await;
                    if let Some(el) = stream_ref.get_untracked() {
                        el.set_scroll_top(el.scroll_height());
                    }
                }
            });
        }
        current
    });

    // The mirror image, for "load more": land on the oldest record.
    Effect::new(move |last: Option<u64>| -> u64 {
        let current = state.with(|s| s.scroll_top_request_counter);
        if last.is_some_and(|prev| prev != current) {
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(0).await;
                if let Some(el) = stream_ref.get_untracked() {
                    el.set_scroll_top(0);
                }
            });
        }
        current
    });

    // The 1 Hz re-pin (mechanism 2 above).
    Effect::new(move |_| {
        clock.track();
        if !state.with_untracked(|s| s.live) {
            return;
        }
        if let Some(el) = stream_ref.get_untracked() {
            if gap_to_bottom(&el) > 1.0 {
                el.set_scroll_top(el.scroll_height());
            }
        }
    });

    let on_scroll = {
        let intent_until = intent_until.clone();
        move |_| {
            let now = js_sys::Date::now();
            if now > intent_until.get() {
                // Layout moved the scroll position, not the user.
                return;
            }
            intent_until.set(now + INTENT_WINDOW_MS);
            if let Some(el) = stream_ref.get() {
                let at_bottom = gap_to_bottom(&el) <= SCROLL_BOTTOM_THRESHOLD;
                // Only write when it would actually change something.
                // A signal write notifies every subscriber unconditionally,
                // and a drag fires this handler once per frame — almost
                // always reporting the state it already reported.
                if state.with_untracked(|s| s.scroll_would_change(at_bottom)) {
                    state.update(|s| s.on_scroll(at_bottom));
                }
            }
        }
    };

    let on_wheel = {
        let mark = mark_intent.clone();
        move |_| mark(INTENT_WINDOW_MS)
    };
    let on_touch_start = {
        let mark = mark_intent.clone();
        move |_| mark(INTENT_WINDOW_MS)
    };
    let on_touch_move = {
        let mark = mark_intent.clone();
        move |_| mark(INTENT_WINDOW_MS)
    };
    let on_key_down = {
        let mark = mark_intent.clone();
        move |e: leptos::ev::KeyboardEvent| {
            if matches!(e.key().as_str(), "PageUp" | "PageDown" | "Home" | "End") {
                mark(INTENT_WINDOW_MS);
            }
        }
    };
    // A press whose target is the container itself — not a row inside it —
    // is a press on the scrollbar.
    let on_pointer_down = move |e: leptos::ev::PointerEvent| {
        let on_container = stream_ref.get_untracked().is_some_and(|el| {
            e.target()
                .is_some_and(|t| t == *AsRef::<web_sys::EventTarget>::as_ref(&el))
        });
        if on_container {
            mark_intent(SCROLLBAR_PRESS_MS);
        }
    };

    let truncated = move || state.with(|s| s.loaded.truncated);
    let is_empty = move || state.with(|s| s.visible_count() == 0);

    view! {
        // `following` switches scroll anchoring to the bottom sentinel. Only
        // while live: paused, the user is reading a row, and pinning the
        // bottom would slide that row away whenever something below it —
        // its own detail pane — changes height.
        <div class="stream"
             class:following=move || state.with(|s| s.live)
             node_ref=stream_ref
             tabindex="0"
             role="log"
             aria-label="Log records"
             on:wheel=on_wheel
             on:touchstart=on_touch_start
             on:touchmove=on_touch_move
             on:keydown=on_key_down
             on:pointerdown=on_pointer_down
             on:scroll=on_scroll>
            <Show when=truncated>
                <TruncatedNote/>
            </Show>
            // Keyed on the record's stable seq, NOT its position. With a
            // positional key, a full buffer (pop_front + push_back) presents
            // an unchanged key set to the reconciler, which then correctly
            // concludes there is nothing to patch — and the rendered rows
            // silently stop tracking the data.
            <For each=rows
                 key=|e: &Entry| e.seq
                 children=move |entry| {
                     view! { <Row entry=entry/> }
                 }/>
            <Show when=is_empty>
                <EmptyState/>
            </Show>
            <div class="stream-anchor"></div>
        </div>
    }
}

/// Shown above the oldest loaded row when the server had more to give.
#[component]
fn TruncatedNote() -> impl IntoView {
    let state = expect_context::<AppCtx>().0;
    let limit = move || state.with(|s| s.loaded.limit);
    let can_load_more = move || limit() < LOAD_ALL_LIMIT;

    view! {
        <div class="stream-note" role="status">
            <span>
                {move || format!(
                    "Showing the newest {} records of {}. Older ones are not loaded.",
                    limit(),
                    state.with(|s| s.time_window.label()),
                )}
            </span>
            <Show when=can_load_more
                  fallback=|| view! {
                      <span class="hint">
                          "Narrow the time range or add search text to reach them."
                      </span>
                  }>
                <button type="button" class="link"
                        on:click=move |_| run_window_fetch(state, LOAD_ALL_LIMIT, FetchAnchor::Oldest)>
                    {format!("Load up to {RECORD_CAP}")}
                </button>
            </Show>
        </div>
    }
}

/// What to say when no row is showing — which depends on *why*.
#[component]
fn EmptyState() -> impl IntoView {
    let state = expect_context::<AppCtx>().0;

    let message = move || {
        state.with(|s| {
            let window = s.time_window.label();
            if s.loading {
                ("Loading…".to_string(), String::new())
            } else if s.needs_server_search() {
                // Checked before "the buffer is empty": an empty buffer that
                // was loaded for a different request says nothing about this
                // one.
                let title = if s.records.is_empty() {
                    "Not searched yet.".to_string()
                } else {
                    format!("None of the {} loaded records match.", s.records.len())
                };
                (
                    title,
                    "Press Enter to search the whole time range.".to_string(),
                )
            } else if s.records.is_empty() {
                let asked_for = s.loaded.filter.describe();
                if s.loaded.filter.matches_nothing() {
                    (
                        "Every level is switched off.".to_string(),
                        "Turn a level button on to see records.".to_string(),
                    )
                } else if !asked_for.is_empty() {
                    (
                        format!("Nothing in {window} matches “{asked_for}”."),
                        "Text is case-sensitive. Check the level buttons, or try a wider time range."
                            .to_string(),
                    )
                } else if s.loaded.filter.levels.is_some() {
                    (
                        format!("No records at the selected levels in {window}."),
                        "Turn on more level buttons, or try a wider time range.".to_string(),
                    )
                } else {
                    (
                        format!("No records in {window}."),
                        "New records appear here as they are ingested.".to_string(),
                    )
                }
            } else {
                (
                    format!("None of the {} loaded records match.", s.records.len()),
                    "Check the level buttons and the filter.".to_string(),
                )
            }
        })
    };

    view! {
        <div class="stream-empty" role="status">
            <div class="title">{move || message().0}</div>
            <div class="hint">{move || message().1}</div>
        </div>
    }
}
