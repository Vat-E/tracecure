use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use sift_api::TailFrame;
use sift_core::Record;
use tokio::sync::broadcast::error::RecvError;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/tail", get(tail))
}

async fn tail(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    let rx = state.store().subscribe();
    ws.on_upgrade(move |socket| handle_tail(socket, rx))
}

async fn handle_tail(socket: WebSocket, mut rx: tokio::sync::broadcast::Receiver<Record>) {
    let (mut sink, mut stream) = socket.split();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.tick().await; // consume the first tick immediately

    loop {
        tokio::select! {
            next = rx.recv() => match next {
                Ok(record) => {
                    if !send_frame(&mut sink, &TailFrame::Record(record)).await {
                        break;
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    let msg = format!("tail lagged by {n} records — reconnect");
                    if !send_frame(&mut sink, &TailFrame::Error { message: msg }).await {
                        break;
                    }
                    // After lag, the receiver is still valid; continue.
                }
                Err(RecvError::Closed) => break,
            },
            _ = heartbeat.tick() => {
                if !send_frame(&mut sink, &TailFrame::Heartbeat).await {
                    break;
                }
            }
            maybe_msg = stream.next() => {
                match maybe_msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        tracing::debug!(%e, "tail client error");
                        break;
                    }
                    // Ignore pings/pongs and any client-sent payload.
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

async fn send_frame(
    sink: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    frame: &TailFrame,
) -> bool {
    match serde_json::to_string(frame) {
        Ok(text) => sink.send(Message::Text(text)).await.is_ok(),
        Err(err) => {
            tracing::error!(%err, "serialize tail frame");
            false
        }
    }
}
