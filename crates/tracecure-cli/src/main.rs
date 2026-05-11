use std::collections::BTreeMap;

use anyhow::Context;

mod tui;
use chrono::Utc;
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use tracecure_api::{IngestResponse, SearchResponse, TailFrame};
use tracecure_core::{Level, Record};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Parser, Debug)]
#[command(name = "sift", version, about = "Sift command-line client")]
struct Cli {
    /// HTTP base URL of the Sift server.
    #[arg(long, default_value = "http://127.0.0.1:7789", global = true)]
    server: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Send a single record.
    Send {
        /// Log body (stdin if omitted).
        body: Option<String>,
        #[arg(long, default_value = "localhost")]
        host: String,
        #[arg(long, default_value = "cli")]
        app: String,
        #[arg(long)]
        stream: Option<String>,
        #[arg(long, default_value = "info")]
        level: String,
    },
    /// Search records by body substring.
    Search {
        query: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Inclusive lower bound, RFC3339 (e.g. 2026-04-23T00:00:00Z).
        #[arg(long)]
        from: Option<chrono::DateTime<chrono::Utc>>,
        /// Exclusive upper bound, RFC3339.
        #[arg(long)]
        to: Option<chrono::DateTime<chrono::Utc>>,
    },
    /// Live-tail records.
    Tail,
    /// Interactive terminal UI with live-tail and filter.
    Tui,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Send {
            body,
            host,
            app,
            stream,
            level,
        } => send(&cli.server, body, host, app, stream, &level).await,
        Cmd::Search {
            query,
            limit,
            from,
            to,
        } => search(&cli.server, &query, limit, from, to).await,
        Cmd::Tail => tail(&cli.server).await,
        Cmd::Tui => tui::run(&cli.server).await,
    }
}

async fn send(
    base: &str,
    body: Option<String>,
    host: String,
    app: String,
    stream: Option<String>,
    level: &str,
) -> anyhow::Result<()> {
    let body = if let Some(b) = body {
        b
    } else {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)?;
        s
    };
    let level: Level = serde_json::from_value(serde_json::Value::String(level.to_string()))
        .with_context(|| format!("unknown level {level:?}"))?;
    let stream_id = stream.unwrap_or_else(|| app.clone());
    let record = Record {
        timestamp: Utc::now(),
        host,
        app,
        stream_id,
        level,
        body,
        labels: BTreeMap::new(),
        fields: BTreeMap::new(),
        raw: None,
    };
    let ndjson = format!("{}\n", serde_json::to_string(&record)?);
    let url = format!("{}/v1/ingest", base.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .post(&url)
        .header("content-type", "application/x-ndjson")
        .body(ndjson)
        .send()
        .await?
        .error_for_status()?;
    let r: IngestResponse = resp.json().await?;
    println!("accepted={} rejected={}", r.accepted, r.rejected);
    Ok(())
}

async fn search(
    base: &str,
    query: &str,
    limit: usize,
    from: Option<chrono::DateTime<chrono::Utc>>,
    to: Option<chrono::DateTime<chrono::Utc>>,
) -> anyhow::Result<()> {
    let url = format!("{}/v1/search", base.trim_end_matches('/'));
    let mut req = reqwest::Client::new()
        .get(&url)
        .query(&[("q", query), ("limit", &limit.to_string())]);
    if let Some(f) = from {
        req = req.query(&[("from", &f.to_rfc3339())]);
    }
    if let Some(t) = to {
        req = req.query(&[("to", &t.to_rfc3339())]);
    }
    let resp: SearchResponse = req.send().await?.error_for_status()?.json().await?;
    for rec in resp.records {
        println!(
            "{ts} {host} {app} {lvl:?} {body}",
            ts = rec.timestamp.to_rfc3339(),
            host = rec.host,
            app = rec.app,
            lvl = rec.level,
            body = rec.body
        );
    }
    eprintln!("(scanned {})", resp.scanned);
    Ok(())
}

async fn tail(base: &str) -> anyhow::Result<()> {
    let ws_url = base
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    let ws_url = format!("{}/v1/tail", ws_url.trim_end_matches('/'));
    let (mut ws, _) = connect_async(&ws_url)
        .await
        .with_context(|| format!("connecting to {ws_url}"))?;
    while let Some(msg) = ws.next().await {
        let msg = msg?;
        match msg {
            Message::Text(text) => match serde_json::from_str::<TailFrame>(&text)? {
                TailFrame::Record(r) => println!(
                    "{ts} {host} {app} {lvl:?} {body}",
                    ts = r.timestamp.to_rfc3339(),
                    host = r.host,
                    app = r.app,
                    lvl = r.level,
                    body = r.body
                ),
                TailFrame::Heartbeat | TailFrame::Unknown => {}
                TailFrame::Error { message } => eprintln!("server error: {message}"),
            },
            Message::Close(_) => break,
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
    // Clean close (server restart or client Ctrl-C) — exit 0. Non-zero exits
    // were surprising to shell pipelines and systemd.
    Ok(())
}
