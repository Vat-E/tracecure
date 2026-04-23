//! WebSocket connection for the live tail with exponential-backoff auto-reconnect.
//! Owns the Status enum and the callback plumbing. Exposes a `RetryHandle` so
//! the UI can force an immediate reconnect while in backoff.

use std::cell::RefCell;
use std::rc::Rc;

use futures::channel::oneshot;
use futures::FutureExt;
use gloo_timers::future::TimeoutFuture;
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Connecting,
    Open,
    /// Waiting to retry. `attempt` starts at 1; `next_in_ms` is the sleep
    /// about to begin (or currently running).
    Reconnecting {
        attempt: u32,
        next_in_ms: u32,
    },
    /// Terminal — not used by the auto-reconnect loop, reserved for future
    /// user-initiated tear-down.
    Closed,
}

impl Status {
    pub fn label(self) -> String {
        match self {
            Self::Connecting => "connecting".into(),
            Self::Open => "connected".into(),
            Self::Reconnecting {
                attempt,
                next_in_ms,
            } => {
                format!(
                    "reconnecting (attempt {}, next in {}s)",
                    attempt,
                    next_in_ms / 1000
                )
            }
            Self::Closed => "disconnected".into(),
        }
    }
    pub fn css(self) -> &'static str {
        match self {
            Self::Open => "status ok",
            Self::Closed => "status err",
            Self::Reconnecting { .. } => "status warn",
            Self::Connecting => "status",
        }
    }
    pub fn is_reconnecting(self) -> bool {
        matches!(self, Self::Reconnecting { .. })
    }
}

/// A handle the caller can use to force an immediate reconnect.
#[derive(Clone)]
pub struct RetryHandle(Rc<RefCell<Option<oneshot::Sender<()>>>>);

impl RetryHandle {
    /// Signal the current backoff sleep to abort early. No-op if the loop
    /// isn't currently sleeping.
    pub fn retry_now(&self) {
        if let Some(tx) = self.0.borrow_mut().take() {
            let _ = tx.send(());
        }
    }
}

pub fn connect(
    on_record: impl Fn(RowRecord) + 'static + Clone,
    status: RwSignal<Status>,
) -> RetryHandle {
    let retry_slot: Rc<RefCell<Option<oneshot::Sender<()>>>> = Rc::new(RefCell::new(None));
    let handle = RetryHandle(retry_slot.clone());

    let status_sig = status;
    let on_rec = on_record;
    wasm_bindgen_futures::spawn_local(async move {
        let mut attempt: u32 = 0;
        loop {
            if attempt == 0 {
                status_sig.set(Status::Connecting);
            }

            let url = build_url();
            let (done_tx, done_rx) = oneshot::channel::<()>();
            let did_open = Rc::new(RefCell::new(false));
            match attach(&url, on_rec.clone(), status_sig, done_tx, did_open.clone()) {
                Ok(()) => {
                    // Wait for the socket to tell us it's dead.
                    let _ = done_rx.await;
                }
                Err(()) => {
                    // WebSocket::new failed outright; fall through to backoff.
                }
            }

            // If the socket ever reached Open state, reset the attempt counter
            // so the next disconnect starts backoff at 1s rather than the
            // stale cap from the previous reconnect storm.
            if *did_open.borrow() {
                attempt = 0;
            }

            // Socket closed. Compute backoff and sleep (cancellable).
            attempt = attempt.saturating_add(1);
            let delay_ms = backoff_ms(attempt);
            status_sig.set(Status::Reconnecting {
                attempt,
                next_in_ms: delay_ms,
            });

            let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
            *retry_slot.borrow_mut() = Some(cancel_tx);
            futures::select! {
                _ = TimeoutFuture::new(delay_ms).fuse() => {}
                _ = cancel_rx.fuse() => {}
            };
            // Clear the slot so retry_now() after this point is a no-op until
            // the next sleep begins.
            retry_slot.borrow_mut().take();
        }
    });

    handle
}

fn build_url() -> String {
    let location = web_sys::window()
        .and_then(|w| w.location().host().ok())
        .unwrap_or_default();
    let proto = web_sys::window()
        .and_then(|w| w.location().protocol().ok())
        .unwrap_or_else(|| "http:".to_string());
    let ws_scheme = if proto == "https:" { "wss" } else { "ws" };
    format!("{ws_scheme}://{location}/v1/tail")
}

fn backoff_ms(attempt: u32) -> u32 {
    let shift = attempt.saturating_sub(1).min(5);
    let base = 1000u32.saturating_mul(1u32 << shift);
    base.min(30_000)
}

/// Open a socket, wire up callbacks, and mark `done_tx` when it closes.
/// Returns Err if construction fails (browsers rarely return Err here;
/// close/error events are delivered asynchronously).
fn attach(
    url: &str,
    on_record: impl Fn(RowRecord) + 'static,
    status: RwSignal<Status>,
    done_tx: oneshot::Sender<()>,
    did_open: Rc<RefCell<bool>>,
) -> Result<(), ()> {
    let ws = WebSocket::new(url).map_err(|_| ())?;

    let done_slot: Rc<RefCell<Option<oneshot::Sender<()>>>> = Rc::new(RefCell::new(Some(done_tx)));

    let on_open = Closure::<dyn FnMut(Event)>::new(move |_| {
        *did_open.borrow_mut() = true;
        status.set(Status::Open);
    });
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

    let slot_for_close = done_slot.clone();
    let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_| {
        if let Some(tx) = slot_for_close.borrow_mut().take() {
            let _ = tx.send(());
        }
    });
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    on_close.forget();

    let slot_for_err = done_slot.clone();
    let on_error = Closure::<dyn FnMut(Event)>::new(move |_| {
        if let Some(tx) = slot_for_err.borrow_mut().take() {
            let _ = tx.send(());
        }
    });
    ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    on_error.forget();

    Ok(())
}
