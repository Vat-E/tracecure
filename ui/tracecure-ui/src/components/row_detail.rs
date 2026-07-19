//! Inline expand detail below a selected row.

use std::sync::Arc;

use leptos::prelude::*;
use tracecure_core::Record;
use tracecure_ui::jsonfmt::{tokenize, JsonColor};
use tracecure_ui::state::Selection;

use crate::app::SelectionCtx;

#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn RowDetail(record: Arc<Record>) -> impl IntoView {
    let selection = expect_context::<SelectionCtx>().0;

    let labels_view = record.labels.iter()
        .map(|(k, v)| {
            let k = k.clone();
            let v = v.clone();
            view! { <div class="field-row"><span class="field-k">{k}</span><span class="field-v">{v}</span></div> }
        })
        .collect::<Vec<_>>();

    // fields JSON via jsonfmt
    let fields_json = serde_json::to_value(&record.fields).unwrap_or(serde_json::Value::Null);
    let tokens = tokenize(&fields_json);
    let fields_view = tokens
        .into_iter()
        .map(|tok| {
            let class = match tok.color {
                JsonColor::Key => "json-k",
                JsonColor::Str => "json-s",
                JsonColor::Num => "json-n",
                JsonColor::Bool => "json-b",
                JsonColor::Null => "json-null",
                JsonColor::Punct => "json-p",
            };
            view! { <span class=class>{tok.text}</span> }
        })
        .collect::<Vec<_>>();

    let raw_text = record.raw.clone().unwrap_or_else(|| "(none)".to_string());

    view! {
        <div class="row-detail">
            <div class="detail-head">
                <span class="detail-title">"Detail"</span>
                <span class="detail-close"
                      on:click=move |_| selection.update(Selection::close)>
                    "×"
                </span>
            </div>
            <div class="detail-sec">
                <div class="field-label">"BODY"</div>
                <div class="field-body">{record.body.clone()}</div>
            </div>
            <div class="detail-sec">
                <div class="field-label">"LABELS"</div>
                <div class="labels">{labels_view}</div>
            </div>
            <div class="detail-sec">
                <div class="field-label">"FIELDS"</div>
                <pre class="json">{fields_view}</pre>
            </div>
            <div class="detail-sec">
                <div class="field-label">"RAW"</div>
                <div class="field-raw">{raw_text}</div>
            </div>
        </div>
    }
}
