//! Live-tail route. Owns the record list signal and renders rows.

use leptos::*;
use serde::Deserialize;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TailFrame {
    Record(TailRecord),
    Heartbeat,
    Error {
        message: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TailRecord {
    pub timestamp: String,
    pub host: String,
    pub app: String,
    #[serde(default)]
    pub level: String,
    pub body: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Open,
    Closed,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Open => "connected",
            Self::Closed => "disconnected",
        }
    }
    pub fn css(self) -> &'static str {
        match self {
            Self::Open => "status ok",
            Self::Closed => "status err",
            Self::Connecting => "status",
        }
    }
}

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

fn connect(records: RwSignal<Vec<TailRecord>>, status: RwSignal<Status>) {
    let location = web_sys::window()
        .and_then(|w| w.location().host().ok())
        .unwrap_or_default();
    let proto = web_sys::window()
        .and_then(|w| w.location().protocol().ok())
        .unwrap_or_else(|| "http:".to_string());
    let ws_scheme = if proto == "https:" { "wss" } else { "ws" };
    let url = format!("{ws_scheme}://{location}/v1/tail");

    let ws = match WebSocket::new(&url) {
        Ok(ws) => ws,
        Err(_) => {
            status.set(Status::Closed);
            return;
        }
    };

    let on_open_status = status;
    let on_open = Closure::<dyn FnMut(Event)>::new(move |_| on_open_status.set(Status::Open));
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();

    let records_for_msg = records;
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
        let Some(text) = evt.data().as_string() else {
            return;
        };
        match serde_json::from_str::<TailFrame>(&text) {
            Ok(TailFrame::Record(r)) => records_for_msg.update(|v| {
                v.push(r);
                if v.len() > 500 {
                    v.drain(..v.len() - 500);
                }
            }),
            Ok(TailFrame::Heartbeat) => {}
            Ok(TailFrame::Error { message }) => log::warn!("server error frame: {message}"),
            Ok(TailFrame::Unknown) => {}
            Err(e) => log::error!("bad frame: {e}"),
        }
    });
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();

    let on_close_status = status;
    let on_close =
        Closure::<dyn FnMut(CloseEvent)>::new(move |_| on_close_status.set(Status::Closed));
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    on_close.forget();

    let on_error_status = status;
    let on_error = Closure::<dyn FnMut(Event)>::new(move |_| on_error_status.set(Status::Closed));
    ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    on_error.forget();
}
