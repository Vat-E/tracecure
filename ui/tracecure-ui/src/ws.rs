//! WebSocket client — connects to /v1/tail, pushes records into `AppState`.
//! Auto-reconnects with exponential backoff (1s → 30s cap).

use std::cell::RefCell;
use std::rc::Rc;

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

pub fn connect_forever(ctx: AppCtx) {
    spawn_local(async move {
        let mut attempt: u32 = 0;
        loop {
            ctx.0.update(|s| s.status = Status::Connecting);

            // Record whether we were already Connected before this attempt
            // (can't happen on first run, but guards the reset below).
            let was_connected_before = ctx.0.with(|s| s.status == Status::Connected);
            let _ = try_connect(ctx).await;

            // If we ever became Connected during this iteration, reset attempt.
            let did_open = ctx.0.with(|s| s.status == Status::Connected) || was_connected_before;
            if did_open {
                attempt = 0;
            } else {
                attempt = (attempt + 1).min(5);
            }

            ctx.0.update(|s| s.status = Status::Disconnected);

            let backoff_ms = (1u32 << attempt).saturating_mul(1000).min(30_000);
            TimeoutFuture::new(backoff_ms).await;
        }
    });
}

async fn try_connect(ctx: AppCtx) -> Result<(), ()> {
    use futures::channel::oneshot;

    let url = ws_url();
    let ws = WebSocket::new(&url).map_err(|_| ())?;

    let (close_tx, close_rx) = oneshot::channel::<()>();
    let close_tx = Rc::new(RefCell::new(Some(close_tx)));

    let ctx_open = ctx;
    let onopen = Closure::<dyn FnMut()>::new(move || {
        ctx_open.0.update(|s| s.status = Status::Connected);
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    // Arrivals are buffered and applied one batch per animation frame. Doing
    // it per message means a full reactive cascade per record.
    let inbox = Rc::new(RefCell::new(Inbox::new(RECORD_CAP)));

    let ctx_msg = ctx;
    let inbox_msg = inbox.clone();
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
                ctx_msg.0.update(|s| s.set_flash_err(message));
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

    // Park until close/error fires. On wake, return Err so the outer
    // backoff loop in connect_forever advances.
    let _ = close_rx.await;
    Err(())
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
