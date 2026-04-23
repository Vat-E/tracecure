//! Search route: query + from/to pickers + quick ranges + results table.

use leptos::*;
use serde::{Deserialize, Serialize};

use crate::components::record_row::{RecordRow, RowRecord};
use crate::components::time_range::{compute_range, QuickRange, TimeRange};

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
    records: Vec<RowRecord>,
    scanned: usize,
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
    let open = use_context::<crate::app::OpenDetail>()
        .expect("OpenDetail ctx")
        .0;
    let on_row_click = Callback::new(move |r: RowRecord| open.set(Some(r)));

    let q = create_rw_signal(String::new());
    let from = create_rw_signal(String::new());
    let to = create_rw_signal(String::new());
    let results = create_rw_signal(Vec::<RowRecord>::new());
    let status = create_rw_signal(Status::Idle);

    let level_ctx = use_context::<crate::app::LevelCtx>().expect("LevelCtx").0;
    let visible_results = move || {
        let lvl = level_ctx.get();
        results
            .get()
            .into_iter()
            .filter(|r| lvl.allows(&r.level))
            .collect::<Vec<_>>()
    };

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
                    results_sig.set(resp.records);
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

    // Auto-populate the last-1h range and kick off an empty search so the user
    // sees recent records immediately when they open the Search tab — instead
    // of staring at an empty results table. Narrowing the range or typing a
    // query rewrites from/to and re-submits.
    let (lo, hi) = compute_range(QuickRange::OneHour);
    from.set(lo);
    to.set(hi);
    submit();

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
                    each={move || visible_results().into_iter().enumerate().collect::<Vec<_>>()}
                    key={|(i, _)| *i}
                    children={move |(_, r)| view! { <RecordRow record=r on_click=on_row_click /> }}
                />
            </div>
        </div>
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

    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;
    if !resp.ok() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("server {status}: {body}"));
    }
    resp.json::<SearchResponse>()
        .await
        .map_err(|e| format!("bad response: {e}"))
}
