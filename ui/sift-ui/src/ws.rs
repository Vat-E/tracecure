//! WebSocket connection for the live tail. Owns the Status enum and the
//! callback plumbing. Auto-reconnect lands in a later task.

use leptos::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

use crate::components::record_row::RowRecord;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TailFrame {
    Record(RowRecord),
    Heartbeat,
    Error {
        message: String,
    },
    #[serde(other)]
    Unknown,
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

pub fn connect(on_record: impl Fn(RowRecord) + 'static, status: RwSignal<Status>) {
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

    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
        let Some(text) = evt.data().as_string() else {
            return;
        };
        match serde_json::from_str::<TailFrame>(&text) {
            Ok(TailFrame::Record(r)) => on_record(r),
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
