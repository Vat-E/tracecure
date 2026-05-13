use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracecure_api::TailFrame;

async fn spawn_server() -> (std::net::SocketAddr, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let state = tracecure_server::AppState::open(tmp.path()).await.unwrap();
    let app = tracecure_server::router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, tmp)
}

#[tokio::test]
async fn tail_receives_ingested_records() {
    let (addr, _tmp) = spawn_server().await;

    let ws_url = format!("ws://{addr}/v1/tail");
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();

    // Post one record via the REST API.
    let client = reqwest::Client::new();
    let line = r#"{"timestamp":"2026-04-22T12:00:00Z","host":"h","app":"a","stream_id":"s","body":"hello tail"}"#;
    let resp = client
        .post(format!("http://{addr}/v1/ingest"))
        .header("content-type", "application/x-ndjson")
        .body(format!("{line}\n"))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Expect a Record frame within 1s.
    let msg = tokio::time::timeout(Duration::from_secs(1), ws.next())
        .await
        .expect("tail frame timeout")
        .expect("stream closed")
        .expect("ws error");
    let text = match msg {
        Message::Text(t) => t,
        Message::Binary(_)
        | Message::Ping(_)
        | Message::Pong(_)
        | Message::Close(_)
        | Message::Frame(_) => {
            panic!("unexpected frame: {msg:?}")
        }
    };
    let frame: TailFrame = serde_json::from_str(&text).unwrap();
    match frame {
        TailFrame::Record(r) => assert_eq!(r.body, "hello tail"),
        other => panic!("expected Record frame, got {other:?}"),
    }

    // Close the socket cleanly so the server doesn't spam warnings.
    let _ = ws.send(Message::Close(None)).await;
}
