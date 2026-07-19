//! WebSocket client — connects to /v1/tail, pushes records into `AppState`.
//! Auto-reconnects with exponential backoff (1s → 30s cap).
//!
//! The loop can also be woken on demand through the reconnect channel, which
//! is what makes the `:retry` command real and lets a server-side lag error
//! re-establish the socket instead of leaving a permanently degraded one.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use futures::channel::mpsc;
use futures::future::{self, Either};
use futures::StreamExt;
use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use tracecure_api::TailFrame;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, WebSocket};

use crate::app::AppCtx;
use tracecure_ui::inbox::Inbox;
use tracecure_ui::state::{Status, RECORD_CAP};

/// Smallest gap between two lag-triggered backfills, so a burst of lag
/// frames cannot turn into a burst of `/v1/search` requests.
const LAG_BACKFILL_COOLDOWN_MS: f64 = 3000.0;

/// How a connection attempt ended, which decides the backoff.
enum Outcome {
    /// The socket opened and later closed. Backoff resets.
    Opened,
    /// The socket never opened. Backoff advances.
    Failed,
    /// The user asked for a reconnect. Retry immediately.
    ReconnectRequested,
}

pub fn connect_forever(ctx: AppCtx, mut reconnect_rx: mpsc::UnboundedReceiver<()>) {
    spawn_local(async move {
        let mut attempt: u32 = 0;
        let mut ever_opened = false;

        loop {
            // Drop any requests queued while we were not listening. A burst
            // of lag errors would otherwise leave tokens in the channel that
            // immediately tear down the socket we are about to open.
            while reconnect_rx.try_recv().is_ok() {}

            ctx.0.update(|s| s.status = Status::Connecting);

            let outcome = try_connect(ctx, &mut reconnect_rx, ever_opened).await;

            ctx.0.update(|s| s.status = Status::Disconnected);

            match outcome {
                Outcome::Opened => {
                    ever_opened = true;
                    attempt = 0;
                }
                Outcome::Failed => attempt = (attempt + 1).min(5),
                // A user asking for a retry should not sit through a backoff.
                Outcome::ReconnectRequested => {
                    attempt = 0;
                    continue;
                }
            }

            let backoff_ms = (1u32 << attempt).saturating_mul(1000).min(30_000);
            TimeoutFuture::new(backoff_ms).await;
        }
    });
}

async fn try_connect(
    ctx: AppCtx,
    reconnect_rx: &mut mpsc::UnboundedReceiver<()>,
    is_reconnect: bool,
) -> Outcome {
    use futures::channel::oneshot;

    let url = ws_url();
    let Ok(ws) = WebSocket::new(&url) else {
        return Outcome::Failed;
    };

    let (close_tx, close_rx) = oneshot::channel::<()>();
    let close_tx = Rc::new(RefCell::new(Some(close_tx)));

    // Tracked locally rather than by re-reading `status`: the loop sets
    // `Connecting` immediately before this runs, so reading the signal back
    // could only ever report "not connected".
    let opened = Rc::new(Cell::new(false));

    let ctx_open = ctx;
    let opened_for_open = opened.clone();
    let onopen = Closure::<dyn FnMut()>::new(move || {
        opened_for_open.set(true);
        ctx_open.0.update(|s| s.status = Status::Connected);

        // Records ingested while the socket was down are in neither the
        // buffer nor this new stream — the tail has no replay. Re-run the
        // backfill to close the gap; merge_backfill dedups and re-sorts, so
        // overlapping with what we already hold is harmless.
        if is_reconnect {
            crate::app::spawn_backfill(ctx_open.0);
        }
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    // Arrivals are buffered and applied one batch per animation frame. Doing
    // it per message means a full reactive cascade per record.
    //
    // The inbox is per-connection, so a reconnect starts clean and a flush
    // scheduled by the old socket cannot mix records into the new one.
    let inbox = Rc::new(RefCell::new(Inbox::new(RECORD_CAP)));

    let ctx_msg = ctx;
    let inbox_msg = inbox.clone();
    let last_lag_backfill = Rc::new(Cell::new(f64::NEG_INFINITY));
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
        let Some(data) = evt.data().as_string() else {
            return;
        };

        // Parse the actual frame envelope. Previously this deserialized a
        // bare `Record`, which happened to work for record frames only
        // because `TailFrame` is internally tagged and `Record` ignores the
        // unknown `"type"` key — and silently discarded everything else,
        // including the server's "tail lagged by N records" warning. That
        // warning fires under precisely the high-volume conditions where
        // dropped records matter most.
        match serde_json::from_str::<TailFrame>(&data) {
            Ok(TailFrame::Record(record)) => {
                // `borrow_mut` cannot collide with the flush below: both run
                // on the single JS thread and neither awaits while borrowed.
                let needs_flush = inbox_msg.borrow_mut().push(record);
                if needs_flush {
                    let inbox_flush = inbox_msg.clone();
                    request_animation_frame(move || {
                        let batch = inbox_flush.borrow_mut().drain();
                        if !batch.is_empty() {
                            ctx_msg.0.update(|s| s.push_batch(batch));
                        }
                    });
                }
            }
            Ok(TailFrame::Error { message }) => {
                // A lag frame means the server's broadcast skipped ahead of
                // this client; it explicitly keeps the receiver valid and
                // carries on, so the socket itself is healthy.
                //
                // Do NOT reconnect here. The overload that caused the lag is
                // still true the instant a new socket opens, so it lags again
                // immediately and the client spends its life in backoff
                // showing nothing — strictly worse than the dropped records.
                // Recover the gap with a backfill and keep the stream.
                ctx_msg.0.update(|s| s.set_flash_err(message));

                let now = js_sys::Date::now();
                if now - last_lag_backfill.get() > LAG_BACKFILL_COOLDOWN_MS {
                    last_lag_backfill.set(now);
                    crate::app::spawn_backfill(ctx_msg.0);
                }
            }
            // Heartbeats keep proxies from idling the socket out; nothing to do.
            Ok(TailFrame::Heartbeat | TailFrame::Unknown) | Err(_) => {}
        }
    });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    // Fire the close channel on either close or error so the outer loop
    // wakes up and retries.
    let close_tx_for_close = close_tx.clone();
    let onclose = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_| {
        if let Some(tx) = close_tx_for_close.borrow_mut().take() {
            let _ = tx.send(());
        }
    });
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();

    let close_tx_for_err = close_tx.clone();
    let onerror = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        if let Some(tx) = close_tx_for_err.borrow_mut().take() {
            let _ = tx.send(());
        }
    });
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    // Park until the socket drops or someone asks for a reconnect.
    let outcome = match future::select(close_rx, reconnect_rx.next()).await {
        Either::Left(_) => {
            if opened.get() {
                Outcome::Opened
            } else {
                Outcome::Failed
            }
        }
        // A closed channel cannot ask for anything; treat it as a plain drop.
        Either::Right((None, _)) => Outcome::Failed,
        Either::Right((Some(()), _)) => Outcome::ReconnectRequested,
    };

    // Always tear the socket down. On the reconnect path it is still open,
    // and leaking it would leave a second live tail feeding a dead handler.
    ws.set_onmessage(None);
    ws.set_onclose(None);
    ws.set_onerror(None);
    let _ = ws.close();

    outcome
}

fn ws_url() -> String {
    let loc = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_else(|| "http://localhost:7789".to_string());
    let ws = loc
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    format!("{ws}/v1/tail")
}
