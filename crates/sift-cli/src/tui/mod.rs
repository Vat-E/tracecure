//! `sift tui` — Ratatui-based terminal UI for live-tail with filter.

mod input;
mod state;
mod view;
mod view_tail;
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
    let ws_handle = tokio::spawn(ws::run(ws::tail_url(server), ws_tx));

    let mut state = AppState::new();
    let mut events = EventStream::new();
    let mut redraw = interval(Duration::from_millis(100));
    redraw.set_missed_tick_behavior(MissedTickBehavior::Skip);

    while !state.should_quit {
        terminal.draw(|f| view::draw(f, &state))?;

        tokio::select! {
            maybe_evt = events.next() => match maybe_evt {
                Some(Ok(Event::Key(k))) if k.kind == KeyEventKind::Press => {
                    input::dispatch(&mut state, k);
                }
                Some(Ok(_)) => {}
                Some(Err(err)) => return Err(err).context("event stream"),
                None => break,
            },
            Some(evt) = ws_rx.recv() => {
                state.apply_ws_event(evt);
            },
            _ = redraw.tick() => {}
        }
    }

    ws_handle.abort();
    let _ = ws_handle.await; // swallow the abort JoinError

    Ok(())
}
