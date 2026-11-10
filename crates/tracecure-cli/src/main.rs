use std::collections::BTreeMap;

use anyhow::Context;

mod tui;
use chrono::Utc;
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracecure_api::{IngestResponse, SearchQuery, SearchResponse, TailFrame};
use tracecure_core::{Level, Record};

#[derive(Parser, Debug)]
#[command(name = "tracecure", version, about = "Tracecure command-line client")]
struct Cli {
    /// HTTP base URL of the Tracecure server.
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
        /// Ingest token, for a server started with `--ingest-auth required`.
        /// Falls back to the `TRACECURE_TOKEN` environment variable, which
        /// keeps the token out of the shell history and the process list.
        #[arg(long)]
        token: Option<String>,
    },
    /// Search records by body substring, optionally narrowed by level, app,
    /// host or stream.
    Search {
        /// Case-sensitive substring of the message. Omit to match any text.
        #[arg(default_value = "")]
        query: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Inclusive lower bound, RFC3339 (e.g. 2026-04-23T00:00:00Z).
        #[arg(long)]
        from: Option<chrono::DateTime<chrono::Utc>>,
        /// Exclusive upper bound, RFC3339.
        #[arg(long)]
        to: Option<chrono::DateTime<chrono::Utc>>,
        /// Only these levels, comma-separated (e.g. error,fatal).
        #[arg(long)]
        level: Option<String>,
        /// Only these apps, comma-separated. A trailing * matches by prefix.
        #[arg(long)]
        app: Option<String>,
        /// Only these hosts, comma-separated. A trailing * matches by prefix.
        #[arg(long)]
        host: Option<String>,
        /// Only these stream ids, comma-separated. A trailing * matches by prefix.
        #[arg(long = "stream-id")]
        stream_id: Option<String>,
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
            token,
        } => send(&cli.server, body, host, app, stream, &level, token).await,
        Cmd::Search {
            query,
            limit,
            from,
            to,
            level,
            app,
            host,
            stream_id,
        } => {
            let query = SearchQuery {
                q: query,
                limit,
                from,
                to,
                level,
                app,
                host,
                stream_id,
            };
            search(&cli.server, &query).await
        }
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
    token: Option<String>,
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
    let mut req = reqwest::Client::new()
        .post(&url)
        .header("content-type", "application/x-ndjson")
        .body(ndjson);
    if let Some(token) = token.or_else(|| std::env::var("TRACECURE_TOKEN").ok()) {
        let token = token.trim().to_string();
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
    }
    let resp = req.send().await?;
    // A 401 says what is wrong (no token, or a revoked one); pass that on.
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!("{}", resp.text().await?);
    }
    let r: IngestResponse = resp.error_for_status()?.json().await?;
    println!("accepted={} rejected={}", r.accepted, r.rejected);
    Ok(())
}

/// The query-string pairs for a search. Only what is set is sent, so a
/// search without filters is the request it always was.
fn search_params(query: &SearchQuery) -> Vec<(&'static str, String)> {
    let mut params = vec![("q", query.q.clone()), ("limit", query.limit.to_string())];
    // The `Z` form, not `to_rfc3339()`'s `+00:00`: see tracecure-api's notes
    // on query-string timestamps.
    let rfc3339 =
        |t: chrono::DateTime<chrono::Utc>| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    if let Some(f) = query.from {
        params.push(("from", rfc3339(f)));
    }
    if let Some(t) = query.to {
        params.push(("to", rfc3339(t)));
    }
    for (key, value) in [
        ("level", &query.level),
        ("app", &query.app),
        ("host", &query.host),
        ("stream_id", &query.stream_id),
    ] {
        if let Some(v) = value {
            params.push((key, v.clone()));
        }
    }
    params
}

async fn search(base: &str, query: &SearchQuery) -> anyhow::Result<()> {
    let url = format!("{}/v1/search", base.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .get(&url)
        .query(&search_params(query))
        .send()
        .await?;
    // A 400 carries the reason (an unknown level name, say); show it rather
    // than a bare status code.
    if resp.status() == reqwest::StatusCode::BAD_REQUEST {
        anyhow::bail!("{}", resp.text().await?);
    }
    let resp: SearchResponse = resp.error_for_status()?.json().await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn a_plain_search_sends_only_q_and_limit() {
        let q = SearchQuery {
            q: "boom".into(),
            limit: 50,
            ..SearchQuery::default()
        };
        assert_eq!(
            search_params(&q),
            vec![("q", "boom".to_string()), ("limit", "50".to_string())]
        );
    }

    #[test]
    fn set_filters_and_bounds_are_sent() {
        let q = SearchQuery {
            q: String::new(),
            limit: 10,
            from: Some(chrono::Utc.with_ymd_and_hms(2026, 4, 23, 0, 0, 0).unwrap()),
            to: None,
            level: Some("error,fatal".into()),
            app: Some("pay*".into()),
            host: None,
            stream_id: Some("api-prod".into()),
        };
        let params = search_params(&q);
        assert!(params.contains(&("level", "error,fatal".to_string())));
        assert!(params.contains(&("app", "pay*".to_string())));
        assert!(params.contains(&("stream_id", "api-prod".to_string())));
        assert!(!params.iter().any(|(k, _)| *k == "host" || *k == "to"));
        // `Z`, never `+00:00`.
        assert!(params.contains(&("from", "2026-04-23T00:00:00.000Z".to_string())));
    }
}
