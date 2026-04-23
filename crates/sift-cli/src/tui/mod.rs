//! `sift tui` — Ratatui-based terminal UI for live-tail with filter.

mod state;
mod view;
mod ws;

use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Context;
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::time::{interval, MissedTickBehavior};

use state::{AppState, Focus};

// used by Task 4's draw function
#[allow(dead_code)]
type Tui = Terminal<CrosstermBackend<Stdout>>;

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
                    handle_key(&mut state, k);
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

fn handle_key(state: &mut AppState, key: KeyEvent) {
    // Ctrl+C always quits.
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        state.quit();
        return;
    }

    match state.focus {
        Focus::List => handle_list_key(state, key),
        Focus::Filter => handle_filter_key(state, key),
    }
}

fn handle_list_key(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') => state.quit(),
        KeyCode::Char('/') => state.focus_filter(),
        KeyCode::Up => state.scroll_up(1),
        KeyCode::Down => state.scroll_down(1),
        KeyCode::PageUp => state.scroll_up(10),
        KeyCode::PageDown => state.scroll_down(10),
        KeyCode::Home => state.scroll_up(usize::MAX / 2),
        KeyCode::End => state.follow(),
        _ => {}
    }
}

fn handle_filter_key(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            state.unfocus_filter();
        }
        KeyCode::Backspace => {
            let mut s = state.filter.clone();
            s.pop();
            state.set_filter(s);
        }
        KeyCode::Char(c) => {
            let mut s = state.filter.clone();
            s.push(c);
            state.set_filter(s);
        }
        _ => {}
    }
}
