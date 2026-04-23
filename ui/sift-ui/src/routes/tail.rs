//! Live-tail route. Consumes the shared `TailCtx` (records + paused + status)
//! from app.rs so its signals survive route switches. The WebSocket is opened
//! in App, not here — Tail only renders the state.

use leptos::*;

use crate::app::{LevelCtx, OpenDetail, TailCtx, TailRetry, TAIL_CAP};
use crate::components::filter_bar::FilterBar;
use crate::components::pause_button::PauseButton;
use crate::components::record_row::{RecordRow, RowRecord};
use crate::components::status_pill::StatusPill;

#[component]
pub fn Tail() -> impl IntoView {
    let tail = use_context::<TailCtx>().expect("TailCtx (provided by App)");
    let retry = use_context::<TailRetry>().expect("TailRetry (provided by App)").0;
    let TailCtx {
        records,
        pending,
        pending_count,
        paused,
        status,
    } = tail;

    // Route-local filter: transient UX state, no reason to persist across tabs.
    let filter = create_rw_signal(String::new());

    let on_resume = Callback::new(move |_: ()| {
        let mut drained: Vec<RowRecord> = Vec::new();
        pending.update(|v| {
            drained = std::mem::take(v);
        });
        records.update(|main| {
            main.extend(drained);
            if main.len() > TAIL_CAP {
                let over = main.len() - TAIL_CAP;
                main.drain(..over);
            }
        });
        pending_count.set(0);
    });

    let level_ctx = use_context::<LevelCtx>().expect("LevelCtx").0;

    // Filter order: level → substring. Both produce the visible set.
    let level_filtered = move || {
        let lvl = level_ctx.get();
        records
            .get()
            .into_iter()
            .filter(|r| lvl.allows(&r.level))
            .collect::<Vec<_>>()
    };
    let visible = move || {
        let needle = filter.get();
        if needle.is_empty() {
            return level_filtered();
        }
        level_filtered()
            .into_iter()
            .filter(|r| r.body.contains(&needle))
            .collect()
    };
    let matches_counter = move || {
        let needle = filter.get();
        if needle.is_empty() {
            return String::new();
        }
        let after_level = level_filtered().len();
        let shown = visible().len();
        let hidden = after_level.saturating_sub(shown);
        format!("{shown} matches · {hidden} filtered out")
    };

    let open = use_context::<OpenDetail>().expect("OpenDetail ctx").0;
    let on_row_click = Callback::new(move |r: RowRecord| open.set(Some(r)));

    view! {
        <div class="tail-header">
            <StatusPill status=status retry=retry.clone() />
            <FilterBar filter=filter />
            <span class="matches-counter">{ matches_counter }</span>
            <PauseButton paused=paused pending_count=pending_count on_resume=on_resume />
        </div>
        { move || {
            let vs = visible();
            if vs.is_empty() {
                view! { <div class="empty">"waiting for records…"</div> }.into_view()
            } else {
                view! {
                    <For
                        each={move || visible().into_iter().enumerate().collect::<Vec<_>>()}
                        key={|(i, _)| *i}
                        children={move |(_, r)| view! {
                            <RecordRow record=r on_click=on_row_click />
                        }}
                    />
                }.into_view()
            }
        }}
    }
}
