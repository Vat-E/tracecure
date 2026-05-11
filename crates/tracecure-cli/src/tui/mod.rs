//! `sift tui` — Ratatui-based terminal UI for live-tail with filter.

mod command;
mod focus;
mod input;
mod jsonfmt;
mod record_input;
mod state;
mod syntax;
mod theme;
mod view;
mod view_detail;
mod view_footer;
mod view_help;
mod view_search;
mod view_search_levels;
mod view_tail;
mod view_tail_middle;
mod ws;

use std::io;
use std::time::Duration;

use anyhow::Context;
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::time::{interval, MissedTickBehavior};

use state::AppState;

/// Guard that restores the terminal on drop, even across panics.
struct TerminalGuard;

impl TerminalGuard {
    fn install() -> anyhow::Result<Self> {
        enable_raw_mode().context("enable_raw_mode")?;
        execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)
            .context("enter alt screen")?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableBracketedPaste);
    }
}

pub async fn run(server: &str) -> anyhow::Result<()> {
    let _guard = TerminalGuard::install()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("terminal init")?;
    terminal.clear().context("terminal clear")?;

    let (ws_tx, mut ws_rx) = tokio::sync::mpsc::channel::<state::WsEvent>(256);
    let (retry_tx, retry_rx) = tokio::sync::mpsc::channel::<()>(4);
    let ws_handle = tokio::spawn(ws::run(ws::tail_url(server), ws_tx, retry_rx));

    let mut state = AppState::new();
    let mut events = EventStream::new();
    let mut redraw = interval(Duration::from_millis(100));
    redraw.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut rate_tick = interval(Duration::from_secs(1));
    rate_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let (search_tx, mut search_rx) = tokio::sync::mpsc::channel::<SearchOutcome>(8);
    let server_base = server.to_string();
    let mut last_seen_running = false;

    while !state.should_quit {
        // Drain any pending command from the `:` palette and dispatch it.
        if let Some(action) = state.pending_command.take() {
            dispatch_command(&mut state, action, &retry_tx);
        }

        terminal.draw(|f| view::draw(f, &state))?;

        tokio::select! {
            maybe_evt = events.next() => match maybe_evt {
                Some(Ok(Event::Key(k))) if k.kind == KeyEventKind::Press => {
                    input::dispatch(&mut state, k, &retry_tx);
                }
                Some(Ok(_)) => {}
                Some(Err(err)) => return Err(err).context("event stream"),
                None => break,
            },
            Some(evt) = ws_rx.recv() => {
                state.apply_ws_event(evt);
            },
            Some(outcome) = search_rx.recv() => {
                match outcome {
                    SearchOutcome::Ok { records, scanned, elapsed_ms } => {
                        state.set_search_results(records, scanned, elapsed_ms);
                    }
                    SearchOutcome::Err(e) => {
                        state.search_status = state::SearchStatus::Err(e);
                    }
                }
                last_seen_running = false;
            },
            _ = rate_tick.tick() => {
                state.rate_tick();
                state.expire_flash_if_due();
            }
            _ = redraw.tick() => {}
        }

        // Fire a new HTTP request whenever the state transitions into Running.
        if matches!(state.search_status, state::SearchStatus::Running) && !last_seen_running {
            last_seen_running = true;
            let tx = search_tx.clone();
            let base = server_base.clone();
            let q = state.search_q.value().to_string();
            let from = state.search_from.value().to_string();
            let to = state.search_to.value().to_string();
            tokio::spawn(async move {
                let _ = tx.send(run_search(&base, &q, &from, &to).await).await;
            });
        }
    }

    ws_handle.abort();
    let _ = ws_handle.await; // swallow the abort JoinError

    Ok(())
}

use tracecure_api::SearchResponse;

enum SearchOutcome {
    Ok {
        records: Vec<tracecure_core::Record>,
        scanned: usize,
        elapsed_ms: u64,
    },
    Err(String),
}

fn dispatch_command(
    state: &mut state::AppState,
    action: command::CommandAction,
    retry_tx: &tokio::sync::mpsc::Sender<()>,
) {
    use command::CommandAction as A;
    match action {
        A::Pause => {
            state.paused = true;
            state.set_flash_ok("paused");
        }
        A::Resume => {
            state.resume();
            state.set_flash_ok("resumed");
        }
        A::Clear => state.clear_records(),
        A::Retry => {
            if retry_tx.try_send(()).is_ok() {
                state.set_flash_ok("retry triggered");
            } else {
                state.set_flash_err("retry already queued");
            }
        }
        A::Save { path } => match state.save_to_path(&path) {
            Ok(()) => {} // flash already set by save_to_path
            Err(e) => state.set_flash_err(format!("save failed: {e}")),
        },
        A::Search { q, from, to } => {
            if let Some(q) = q {
                state.search_q = tui_input::Input::new(q);
            }
            if let Some(f) = from {
                state.search_from =
                    tui_input::Input::new(f.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
            }
            if let Some(t) = to {
                state.search_to =
                    tui_input::Input::new(t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
            }
            state.set_mode(state::Mode::Search);
            state.search_status = state::SearchStatus::Running;
            state.set_flash_ok("search submitted");
        }
        A::Level { level, on } => {
            let current = state.level_set.allows(level);
            if current != on {
                state.toggle_level(level);
            }
            state.set_flash_ok(format!("{level:?} {}", if on { "on" } else { "off" }));
        }
        A::Help => {
            state.help_open = true;
        }
        A::Quit => state.quit(),
    }
}

async fn run_search(base: &str, q: &str, from: &str, to: &str) -> SearchOutcome {
    let started = std::time::Instant::now();
    let url = format!("{}/v1/search", base.trim_end_matches('/'));
    let mut req = reqwest::Client::new()
        .get(&url)
        .query(&[("q", q), ("limit", "100")]);
    if !from.is_empty() {
        req = req.query(&[("from", from)]);
    }
    if !to.is_empty() {
        req = req.query(&[("to", to)]);
    }
    match req
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
    {
        Ok(resp) => match resp.json::<SearchResponse>().await {
            Ok(r) => SearchOutcome::Ok {
                records: r.records,
                scanned: r.scanned,
                // u128 → u64 truncation is fine for "elapsed ms of an HTTP
                // request" — we'd need the process to run for ~584 million
                // years to overflow.
                elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            },
            Err(e) => SearchOutcome::Err(e.to_string()),
        },
        Err(e) => SearchOutcome::Err(e.to_string()),
    }
}
