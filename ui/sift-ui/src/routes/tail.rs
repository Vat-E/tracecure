//! Live-tail route. Owns the record list signal; delegates connection to ws.rs.

use leptos::*;

use crate::app::OpenDetail;
use crate::components::record_row::{RecordRow, RowRecord};
use crate::ws::{connect, Status};

#[component]
pub fn Tail() -> impl IntoView {
    let records = create_rw_signal(Vec::<RowRecord>::new());
    let status = create_rw_signal(Status::Connecting);
    connect(records, status);

    let open = use_context::<OpenDetail>().expect("OpenDetail ctx").0;
    let on_row_click = Callback::new(move |r: RowRecord| open.set(Some(r)));

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
                            <RecordRow record=r on_click=on_row_click />
                        }}
                    />
                }.into_view()
            }
        }}
    }
}
