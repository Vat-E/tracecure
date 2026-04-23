//! WebSocket tail task for the TUI with exponential-backoff auto-reconnect.
//! Forwards `WsEvent`s into an `mpsc::Sender` and listens for retry nudges
//! on a second channel so the user can force an immediate reconnect.

use std::time::Duration;

use futures_util::StreamExt;
use sift_api::TailFrame;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use super::state::{Status, WsEvent};

/// Run the connect/retry loop indefinitely. Exits only when `tx` is dropped.
pub async fn run(ws_url: String, tx: mpsc::Sender<WsEvent>, mut retry_rx: mpsc::Receiver<()>) {
    let mut attempt: u32 = 0;

    loop {
        if attempt == 0 {
            let _ = tx.send(WsEvent::Status(Status::Connecting)).await;
        }

        match connect_async(&ws_url).await {
            Ok((mut ws, _)) => {
                // Successful connect — reset the attempt counter immediately
                // so a reconnect-then-disconnect doesn't start at a stale
                // attempt count.
                attempt = 0;
                let _ = tx.send(WsEvent::Status(Status::Connected)).await;

                // Forward frames until the socket closes or errors.
                while let Some(msg) = ws.next().await {
                    let Ok(msg) = msg else { break };
                    let text = match msg {
                        Message::Text(t) => t,
                        Message::Close(_) => break,
                        Message::Binary(_)
                        | Message::Ping(_)
                        | Message::Pong(_)
                        | Message::Frame(_) => continue,
                    };
                    let Ok(frame) = serde_json::from_str::<TailFrame>(&text) else {
                        continue;
                    };
                    // Surface server-sent error frames (e.g. "tail lagged by
                    // N records") via tracing so the operator can see them
                    // even though the state layer drops them silently.
                    if let TailFrame::Error { ref message } = frame {
                        tracing::warn!(%message, "server reported tail error");
                    }
                    if tx.send(WsEvent::Frame(frame)).await.is_err() {
                        return; // UI dropped; stop cleanly.
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "websocket connect failed");
            }
        }

        // Socket is dead. Compute backoff and sleep, but allow retry nudges.
        attempt = attempt.saturating_add(1);
        let delay_ms = backoff_ms(attempt);
        if tx
            .send(WsEvent::Status(Status::Reconnecting {
                attempt,
                next_in_ms: delay_ms,
            }))
            .await
            .is_err()
        {
            return;
        }

        // Drain anything already queued on retry_rx so we don't fire twice.
        while retry_rx.try_recv().is_ok() {}

        // Wait either for the timeout or for a retry nudge, whichever comes first.
        let _ = timeout(Duration::from_millis(u64::from(delay_ms)), retry_rx.recv()).await;
    }
}

fn backoff_ms(attempt: u32) -> u32 {
    let shift = attempt.saturating_sub(1).min(5);
    let base = 1000u32.saturating_mul(1u32 << shift);
    base.min(30_000)
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

    #[test]
    fn backoff_caps_at_30_seconds() {
        assert_eq!(backoff_ms(1), 1_000);
        assert_eq!(backoff_ms(2), 2_000);
        assert_eq!(backoff_ms(3), 4_000);
        assert_eq!(backoff_ms(4), 8_000);
        assert_eq!(backoff_ms(5), 16_000);
        assert_eq!(backoff_ms(6), 30_000);
        assert_eq!(backoff_ms(100), 30_000);
    }
}
