//! Live-tail route. Owns the record list signal; delegates connection to ws.rs.

use leptos::*;

use crate::ws::{connect, Status, TailRecord};

#[component]
pub fn Tail() -> impl IntoView {
    let records = create_rw_signal(Vec::<TailRecord>::new());
    let status = create_rw_signal(Status::Connecting);
    connect(records, status);

    view! {
        <div class="tail-status-line">
            <span class=move || status.get().css()>
                { move || status.get().label() }
            </span>
        </div>
        { move || {
            let rs = records.get();
            if rs.is_empty() {
                view! { <div class="empty">"waiting for records…"</div> }.into_view()
            } else {
                view! {
                    <For
                        each={move || records.get().into_iter().enumerate().collect::<Vec<_>>()}
                        key={|(i, _)| *i}
                        children={move |(_, r)| view! {
                            <div class="row">
                                <span class="ts">{r.timestamp.clone()}</span>
                                <span class="host">{r.host.clone()}</span>
                                <span class="app">{r.app.clone()}</span>
                                <span class=format!("lvl {}", r.level)>{r.level.clone()}</span>
                                <span class="body">{r.body.clone()}</span>
                            </div>
                        }}
                    />
                }.into_view()
            }
        }}
    }
}
