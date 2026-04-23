//! WebSocket tail task for the TUI. Connects, decodes frames, and emits
//! `WsEvent`s into an `mpsc::Sender`. Does not reconnect — on disconnect it
//! emits `WsEvent::Disconnected` and exits.

use futures_util::StreamExt;
use sift_api::TailFrame;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use super::state::{Status, WsEvent};

/// Connect to `ws_url` and forward frames into `tx` until the socket closes
/// or an error occurs. Always ends by sending `WsEvent::Disconnected` so the
/// UI reflects the terminal state (unless the channel is already closed).
pub async fn run(ws_url: String, tx: mpsc::Sender<WsEvent>) {
    let _ = tx.send(WsEvent::Status(Status::Connecting)).await;
    let Ok((mut ws, _)) = connect_async(&ws_url).await else {
        let _ = tx.send(WsEvent::Disconnected).await;
        return;
    };
    let _ = tx.send(WsEvent::Status(Status::Connected)).await;

    while let Some(msg) = ws.next().await {
        let Ok(msg) = msg else { break };
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {
                continue;
            }
        };
        let Ok(frame) = serde_json::from_str::<TailFrame>(&text) else {
            continue;
        };
        if tx.send(WsEvent::Frame(frame)).await.is_err() {
            return; // UI dropped; stop cleanly.
        }
    }

    let _ = tx.send(WsEvent::Disconnected).await;
}

/// Rewrite an HTTP base URL to a WebSocket base URL and append `/v1/tail`.
#[must_use]
pub fn tail_url(server: &str) -> String {
    let base = server
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    format!("{}/v1/tail", base.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_url_rewrites_http_to_ws() {
        assert_eq!(
            tail_url("http://127.0.0.1:7789"),
            "ws://127.0.0.1:7789/v1/tail"
        );
    }

    #[test]
    fn tail_url_rewrites_https_to_wss() {
        assert_eq!(
            tail_url("https://logs.example.com"),
            "wss://logs.example.com/v1/tail"
        );
    }

    #[test]
    fn tail_url_strips_trailing_slash() {
        assert_eq!(
            tail_url("http://127.0.0.1:7789/"),
            "ws://127.0.0.1:7789/v1/tail"
        );
    }
}
