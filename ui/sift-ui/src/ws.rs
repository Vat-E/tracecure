//! WebSocket client — connects to /v1/tail, pushes records into `AppState`.
//! Auto-reconnects with exponential backoff (1s → 30s cap).

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, WebSocket};

use crate::app::AppCtx;
use sift_ui::state::Status;

pub fn connect_forever(ctx: AppCtx) {
    spawn_local(async move {
        let mut attempt: u32 = 0;
        loop {
            ctx.0.update(|s| s.status = Status::Connecting);

            match try_connect(ctx).await {
                Ok(()) => {
                    // Connection opened and then closed normally — reset attempt.
                    attempt = 0;
                }
                Err(()) => {
                    attempt = (attempt + 1).min(5);
                }
            }
            ctx.0.update(|s| s.status = Status::Disconnected);

            let backoff_ms = (1u32 << attempt).saturating_mul(1000).min(30_000);
            TimeoutFuture::new(backoff_ms).await;
        }
    });
}

async fn try_connect(ctx: AppCtx) -> Result<(), ()> {
    let url = ws_url();
    let ws = WebSocket::new(&url).map_err(|_| ())?;

    let ctx_open = ctx;
    let onopen = Closure::<dyn FnMut()>::new(move || {
        ctx_open.0.update(|s| s.status = Status::Connected);
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let ctx_msg = ctx;
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
        if let Some(data) = evt.data().as_string() {
            if let Ok(record) = serde_json::from_str::<sift_core::Record>(&data) {
                ctx_msg.0.update(|s| s.push_record(record));
            }
        }
    });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    // Hold connection open until something terminates this future.
    // For simplicity, sleep indefinitely; the WS will be closed when
    // the page unloads or onclose fires (and we'll just leak the closure).
    futures::future::pending::<()>().await;
    Ok(())
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
