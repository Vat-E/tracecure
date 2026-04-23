//! Live-tail route. Owns the record list signal; delegates connection to ws.rs.

use leptos::*;

use crate::app::{LevelCtx, OpenDetail};
use crate::components::filter_bar::FilterBar;
use crate::components::pause_button::PauseButton;
use crate::components::record_row::{RecordRow, RowRecord};
use crate::components::status_pill::StatusPill;
use crate::ws::{connect, Status};

const TAIL_CAP: usize = 500;

#[component]
pub fn Tail() -> impl IntoView {
    let records = create_rw_signal(Vec::<RowRecord>::new());
    let pending = create_rw_signal(Vec::<RowRecord>::new());
    let pending_count = create_rw_signal(0usize);
    let paused = create_rw_signal(false);
    let status = create_rw_signal(Status::Connecting);
    let filter = create_rw_signal(String::new());

    // Route new records: when paused, queue in pending; when live, append to main list.
    let on_record = move |r: RowRecord| {
        if paused.get_untracked() {
            pending.update(|v| {
                v.push(r);
                if v.len() > TAIL_CAP {
                    v.drain(..v.len() - TAIL_CAP);
                }
            });
            pending_count.set(pending.with(|v| v.len()));
        } else {
            records.update(|v| {
                v.push(r);
                if v.len() > TAIL_CAP {
                    v.drain(..v.len() - TAIL_CAP);
                }
            });
        }
    };
    let retry = connect(on_record, status);

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
