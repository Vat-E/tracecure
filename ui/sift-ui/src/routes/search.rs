//! Search route: query + from/to pickers + quick ranges + results table.

use leptos::*;
use serde::{Deserialize, Serialize};

use crate::components::record_row::{RecordRow, RowRecord};
use crate::components::time_range::{QuickRange, TimeRange};

#[derive(Clone, Serialize, Debug)]
struct SearchQuery {
    q: String,
    limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
}

#[derive(Clone, Deserialize, Debug)]
struct SearchResponse {
    records: Vec<ServerRecord>,
    scanned: usize,
}

#[derive(Clone, Deserialize, Debug)]
struct ServerRecord {
    timestamp: String,
    host: String,
    app: String,
    #[serde(default)]
    level: String,
    body: String,
    // labels/fields/raw ignored for the results table; detail panel loads them.
}

#[derive(Clone, Debug)]
enum Status {
    Idle,
    Running,
    Ok { scanned: usize, elapsed_ms: f64 },
    Err(String),
}

#[component]
pub fn Search() -> impl IntoView {
    let q = create_rw_signal(String::new());
    let from = create_rw_signal(String::new());
    let to = create_rw_signal(String::new());
    let results = create_rw_signal(Vec::<RowRecord>::new());
    let status = create_rw_signal(Status::Idle);

    let submit = move || {
        let req = SearchQuery {
            q: q.get(),
            limit: 100,
            from: to_rfc3339(&from.get()),
            to: to_rfc3339(&to.get()),
        };
        results.set(Vec::new());
        status.set(Status::Running);

        let status_sig = status;
        let results_sig = results;
        wasm_bindgen_futures::spawn_local(async move {
            let started = js_sys::Date::now();
            match run_search(req).await {
                Ok(resp) => {
                    let elapsed = js_sys::Date::now() - started;
                    results_sig.set(resp.records.into_iter().map(row_from_server).collect());
                    status_sig.set(Status::Ok {
                        scanned: resp.scanned,
                        elapsed_ms: elapsed,
                    });
                }
                Err(e) => status_sig.set(Status::Err(e)),
            }
        });
    };

    let on_quick = Callback::new(move |_q: QuickRange| {
        submit();
    });

    view! {
        <div class="search-page">
            <div class="search-form">
                <input
                    type="text"
                    class="query"
                    placeholder="substring…"
                    prop:value=move || q.get()
                    on:input=move |ev| q.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" { submit(); }
                    }
                />
                <button type="button" class="submit" on:click=move |_| submit()>"Search"</button>
            </div>
            <TimeRange from=from to=to on_quick=on_quick />
            <div class="status-line">
                { move || match status.get() {
                    Status::Idle => "".to_string(),
                    Status::Running => "searching…".to_string(),
                    Status::Ok { scanned, elapsed_ms } => format!(
                        "{} results · scanned {} · {} ms",
                        results.get().len(), scanned, elapsed_ms as i64
                    ),
                    Status::Err(e) => format!("error: {e}"),
                }}
            </div>
            <div class="search-results">
                <For
                    each={move || results.get().into_iter().enumerate().collect::<Vec<_>>()}
                    key={|(i, _)| *i}
                    children={move |(_, r)| view! { <RecordRow record=r /> }}
                />
            </div>
        </div>
    }
}

fn row_from_server(r: ServerRecord) -> RowRecord {
    RowRecord {
        timestamp: r.timestamp,
        host: r.host,
        app: r.app,
        level: r.level,
        body: r.body,
    }
}

/// Convert a datetime-local string ("2026-04-23T12:00" or "2026-04-23T12:00:00")
/// into RFC3339 UTC ("2026-04-23T12:00:00Z"). Returns None for empty input.
fn to_rfc3339(v: &str) -> Option<String> {
    if v.is_empty() {
        return None;
    }
    // datetime-local gives us either minute or second precision; we append
    // :00 if needed and always append Z.
    let core = if v.len() == 16 {
        format!("{v}:00")
    } else {
        v.to_string()
    };
    Some(format!("{core}Z"))
}

async fn run_search(req: SearchQuery) -> Result<SearchResponse, String> {
    let mut url = "/v1/search".to_string();
    let mut first = true;
    let mut push = |k: &str, v: &str| {
        let sep = if first { '?' } else { '&' };
        first = false;
        url.push(sep);
        let encoded = js_sys::encode_uri_component(v)
            .as_string()
            .unwrap_or_default();
        url.push_str(&format!("{k}={encoded}"));
    };
    push("q", &req.q);
    push("limit", &req.limit.to_string());
    if let Some(f) = &req.from {
        push("from", f);
    }
    if let Some(t) = &req.to {
        push("to", t);
    }

    gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<SearchResponse>()
        .await
        .map_err(|e| e.to_string())
}
