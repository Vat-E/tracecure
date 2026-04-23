//! `sift tui` — Ratatui-based terminal UI for live-tail with filter.

mod state;
mod view;

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

pub async fn run(_server: &str) -> anyhow::Result<()> {
    let _guard = TerminalGuard::install()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("terminal init")?;
    terminal.clear().context("terminal clear")?;

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
            _ = redraw.tick() => {}
        }
    }

    Ok(())
}

fn handle_key(state: &mut AppState, key: KeyEvent) {
    // Global quit bindings: Ctrl+C always quits; `q` quits only when the
    // list has focus (filter focus interprets `q` as a literal character).
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        state.quit();
        return;
    }
    if let (Focus::List, KeyCode::Char('q')) = (state.focus, key.code) {
        state.quit();
    }
}
