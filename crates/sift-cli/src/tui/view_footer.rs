//! One-row footer. Renders either keybind hints, the `:` command prompt,
//! or a transient flash message — plus a right-aligned mode badge.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::focus::Pane;
use super::state::{AppState, FlashKind, Mode};

pub fn draw_footer(f: &mut Frame, area: Rect, state: &AppState) {
    let [left, right] = Layout::horizontal([Constraint::Min(1), Constraint::Length(5)]).areas(area);

    render_left(f, left, state);
    render_mode_badge(f, right, state);
}

fn render_left(f: &mut Frame, area: Rect, state: &AppState) {
    if state.command_open {
        let line = Line::from(vec![
            Span::styled(
                " : ",
                Style::default()
                    .fg(state.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(state.command_input.value().to_string()),
        ]);
        f.render_widget(Paragraph::new(line), area);
        // Position the terminal cursor at the input's visible position.
        let x = area.x + 3 + u16::try_from(state.command_input.visual_cursor()).unwrap_or(0);
        f.set_cursor_position((x, area.y));
        return;
    }

    if let Some(flash) = &state.flash {
        let fg = match flash.kind {
            FlashKind::Ok => state.theme.flash_ok,
            FlashKind::Err => state.theme.flash_err,
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                format!(" {} ", flash.msg),
                Style::default().fg(fg).add_modifier(Modifier::BOLD),
            )),
            area,
        );
        return;
    }

    f.render_widget(Paragraph::new(hints_for(state)), area);
}

fn hints_for(state: &AppState) -> Line<'static> {
    let key = Style::default().fg(state.theme.shortcut_key);
    let lbl = Style::default().fg(state.theme.shortcut_label);
    let sep = Span::styled(" · ", Style::default().fg(state.theme.muted));

    let hints: Vec<(&'static str, &'static str)> = match state.mode {
        Mode::Tail => vec![
            ("/", "filter"),
            ("Space", "pause"),
            ("\u{2191}\u{2193}", "nav"),
            ("\u{21B5}", "detail"),
            ("s", "search"),
            (":", "cmd"),
            ("?", "help"),
            ("q", "quit"),
        ],
        Mode::Search => vec![
            ("Tab", "next"),
            ("\u{21B5}", "search"),
            ("o", "detail"),
            ("t", "tail"),
            (":", "cmd"),
            ("?", "help"),
        ],
    };

    let mut spans: Vec<Span<'static>> = vec![Span::raw(" ")];
    for (i, (k, l)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(sep.clone());
        }
        spans.push(Span::styled(*k, key));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(*l, lbl));
    }
    Line::from(spans)
}

fn render_mode_badge(f: &mut Frame, area: Rect, state: &AppState) {
    let badge = if state.command_open {
        "[C]"
    } else if is_insert_mode(state) {
        "[I]"
    } else {
        "[N]"
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            badge,
            Style::default()
                .fg(state.theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(ratatui::layout::Alignment::Right),
        area,
    );
}

fn is_insert_mode(state: &AppState) -> bool {
    matches!(
        state.focus_pane,
        Pane::Filter | Pane::SearchQuery | Pane::SearchFrom | Pane::SearchTo
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_into_buffer(state: &AppState) -> String {
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw_footer(f, f.area(), state)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect::<String>()
    }

    #[test]
    fn normal_mode_shows_hints_and_n_badge() {
        let s = AppState::new();
        let out = render_into_buffer(&s);
        assert!(out.contains("filter"));
        assert!(out.contains("[N]"));
    }

    #[test]
    fn command_mode_shows_colon_prompt_and_c_badge() {
        let mut s = AppState::new();
        s.open_command_prompt();
        s.command_input = tui_input::Input::new("pa".into());
        let out = render_into_buffer(&s);
        assert!(out.contains(':'));
        assert!(out.contains("pa"));
        assert!(out.contains("[C]"));
    }

    #[test]
    fn flash_err_shows_message_instead_of_hints() {
        let mut s = AppState::new();
        s.set_flash_err("boom");
        let out = render_into_buffer(&s);
        assert!(out.contains("boom"));
        assert!(!out.contains("filter"));
    }

    #[test]
    fn insert_mode_shows_i_badge() {
        let mut s = AppState::new();
        s.focus_pane = Pane::Filter;
        let out = render_into_buffer(&s);
        assert!(out.contains("[I]"));
    }
}
