//! Inline expand detail below a selected row.

use std::sync::Arc;

use leptos::prelude::*;
use tracecure_core::Record;
use tracecure_ui::filter::with_token;
use tracecure_ui::jsonfmt::{tokenize, JsonColor};
use tracecure_ui::state::Selection;
use tracecure_ui::timefmt::full_local;
use wasm_bindgen::{JsCast, JsValue};

use crate::app::{AppCtx, SelectionCtx, TzCtx};
use crate::components::row::level_str;

#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn RowDetail(record: Arc<Record>) -> impl IntoView {
    let state = expect_context::<AppCtx>().0;
    let selection = expect_context::<SelectionCtx>().0;
    let tz = expect_context::<TzCtx>().0;

    let labels_view = record.labels.iter()
        .map(|(k, v)| {
            let k = k.clone();
            let v = v.clone();
            view! { <div class="field-row"><span class="field-k">{k}</span><span class="field-v">{v}</span></div> }
        })
        .collect::<Vec<_>>();
    let has_labels = !record.labels.is_empty();
    let has_fields = !record.fields.is_empty();

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

    let raw = record.raw.clone();

    // Both the local time the row shows and the exact UTC instant: the
    // first is what the reader thinks in, the second is what to paste into
    // another tool or a `:search from=`.
    let local_time = full_local(&record.timestamp, tz);
    let utc_time = record
        .timestamp
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let copy = move |what: &'static str, text: String| {
        move |_| {
            let ok = copy_to_clipboard(&text);
            state.update(|s| {
                if ok {
                    s.set_flash_ok(format!("{what} copied"));
                } else {
                    s.set_flash_err("clipboard unavailable");
                }
            });
        }
    };
    let copy_body = copy("message", record.body.clone());
    let copy_json = copy(
        "record JSON",
        serde_json::to_string_pretty(&*record).unwrap_or_default(),
    );

    // Narrow the view to this record's app / host / stream. A value that
    // cannot be written as a filter token (it has a space) gets no button
    // rather than one that filters on something else.
    let filter_button = move |key: &'static str, value: &str| {
        let value = value.to_string();
        let expressible = with_token("", key, &value).is_some();
        expressible.then(|| {
            let title = format!("Show only {key}:{value}");
            view! {
                <button type="button" class="chip-btn" title=title
                        on:click=move |_| {
                            let q = state.with_untracked(|s| with_token(s.query(), key, &value));
                            if let Some(q) = q {
                                state.update(|s| s.set_query(q));
                            }
                        }>
                    "only this"
                </button>
            }
        })
    };

    view! {
        <div class="row-detail">
            <div class="detail-head">
                <span class="detail-title">"Detail"</span>
                <button type="button" class="chip-btn" on:click=copy_body>"copy message"</button>
                <button type="button" class="chip-btn" on:click=copy_json>"copy JSON"</button>
                <button type="button" class="detail-close" aria-label="Close detail"
                        on:click=move |_| selection.update(Selection::close)>
                    "×"
                </button>
            </div>
            <div class="detail-sec meta">
                <div class="field-row">
                    <span class="field-k">"time"</span>
                    <span class="field-v">{local_time}</span>
                    <span class="field-dim">{utc_time}</span>
                </div>
                <div class="field-row">
                    <span class="field-k">"level"</span>
                    <span class="field-v">{level_str(record.level)}</span>
                </div>
                <div class="field-row">
                    <span class="field-k">"app"</span>
                    <span class="field-v">{record.app.clone()}</span>
                    {filter_button("app", &record.app)}
                </div>
                <div class="field-row">
                    <span class="field-k">"host"</span>
                    <span class="field-v">{record.host.clone()}</span>
                    {filter_button("host", &record.host)}
                </div>
                <div class="field-row">
                    <span class="field-k">"stream"</span>
                    <span class="field-v">{record.stream_id.clone()}</span>
                    {filter_button("stream", &record.stream_id)}
                </div>
            </div>
            <div class="detail-sec">
                <div class="field-label">"MESSAGE"</div>
                <div class="field-body">{record.body.clone()}</div>
            </div>
            {has_labels.then(|| view! {
                <div class="detail-sec">
                    <div class="field-label">"LABELS"</div>
                    <div class="labels">{labels_view}</div>
                </div>
            })}
            {has_fields.then(|| view! {
                <div class="detail-sec">
                    <div class="field-label">"FIELDS"</div>
                    <pre class="json">{fields_view}</pre>
                </div>
            })}
            {raw.map(|raw| view! {
                <div class="detail-sec">
                    <div class="field-label">"RAW"</div>
                    <div class="field-raw">{raw}</div>
                </div>
            })}
        </div>
    }
}

/// `navigator.clipboard.writeText(text)`, looked up dynamically.
///
/// Dynamic because the Clipboard API is absent on an insecure origin (plain
/// HTTP that is not localhost) — a normal way to run a self-hosted tool —
/// and that must degrade to a message, not a panic. Returns whether the
/// call could be made; the write itself is asynchronous.
fn copy_to_clipboard(text: &str) -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let get = |obj: &JsValue, key: &str| {
        js_sys::Reflect::get(obj, &JsValue::from_str(key))
            .ok()
            .filter(|v| !v.is_undefined() && !v.is_null())
    };
    let Some(clipboard) = get(&window, "navigator").and_then(|nav| get(&nav, "clipboard")) else {
        return false;
    };
    let Some(write_text) =
        get(&clipboard, "writeText").and_then(|f| f.dyn_into::<js_sys::Function>().ok())
    else {
        return false;
    };
    write_text
        .call1(&clipboard, &JsValue::from_str(text))
        .is_ok()
}
