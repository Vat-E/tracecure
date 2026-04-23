//! Shared row renderer used by both tail and search result lists.
//!
//! Records arrive as a generic struct with the fields every row needs. We
//! don't depend on sift_api::Record here because the UI is wasm32 and the
//! workspace crate isn't compiled for that target.

use leptos::*;

#[derive(Clone, PartialEq, Debug)]
pub struct RowRecord {
    pub timestamp: String,
    pub host: String,
    pub app: String,
    pub level: String,
    pub body: String,
}

#[component]
pub fn RecordRow(
    #[prop(into)] record: RowRecord,
    /// Called when the row is clicked; used by the detail panel.
    #[prop(optional)]
    on_click: Option<Callback<RowRecord>>,
) -> impl IntoView {
    let body_truncated = {
        let b = &record.body;
        if b.len() > 200 {
            let mut end = 200;
            while !b.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…", &b[..end])
        } else {
            b.clone()
        }
    };
    let rec_for_click = record.clone();
    view! {
        <div
            class="row clickable"
            on:click=move |_| {
                if let Some(cb) = on_click {
                    cb.call(rec_for_click.clone());
                }
            }
        >
            <span class="ts">{record.timestamp.clone()}</span>
            <span class="host">{record.host.clone()}</span>
            <span class="app">{record.app.clone()}</span>
            <span class=format!("lvl {}", record.level)>{record.level.clone()}</span>
            <span class="body">{body_truncated}</span>
        </div>
    }
}
