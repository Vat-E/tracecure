//! Centered 80%×80% popup over the main view. Shows full record details with
//! nested bordered sections and syntect JSON highlighting for `fields`.
//! Falls back to `jsonfmt::highlight` if syntect fails for any reason.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use tracecure_core::Record;

use super::jsonfmt;
use super::syntax;
use super::theme::Theme;

pub fn draw_detail(f: &mut Frame, r: &Record) {
    let theme = Theme::dark();
    draw_detail_with_theme(f, r, &theme);
}

fn draw_detail_with_theme(f: &mut Frame, r: &Record, theme: &Theme) {
    let area = centered_rect(80, 80, f.area());
    f.render_widget(Clear, area);

    let title = detail_title(r, theme);
    let close = Line::from(" × ").right_aligned();

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focused))
        .title(title)
        .title(close);

    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let [header, body_area, labels_area, fields_area, raw_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(4),
        Constraint::Length(6),
        Constraint::Min(4),
        Constraint::Length(4),
    ])
    .areas(inner);

    render_header(f, header, r, theme);
    render_body(f, body_area, r, theme);
    render_labels(f, labels_area, r, theme);
    render_fields(f, fields_area, r, theme);
    render_raw(f, raw_area, r, theme);
}

fn detail_title(r: &Record, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::raw(" record · "),
        Span::styled(r.timestamp.to_rfc3339(), Style::default().fg(theme.muted)),
        Span::raw(" · "),
        Span::raw(r.host.clone()),
        Span::raw(" · "),
        Span::raw(r.app.clone()),
        Span::raw(" · "),
        Span::styled(
            format!("{:?}", r.level),
            Style::default()
                .fg(theme.level_color(r.level))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ])
}

fn render_header(_f: &mut Frame, _area: Rect, _r: &Record, _theme: &Theme) {
    // Reserved row — title carries the record identity.
}

fn render_body(f: &mut Frame, area: Rect, r: &Record, theme: &Theme) {
    let block = section_block(" body ", theme);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(r.body.clone())
            .style(Style::default().fg(theme.fg))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_labels(f: &mut Frame, area: Rect, r: &Record, theme: &Theme) {
    let block = section_block(" labels ", theme);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if r.labels.is_empty() {
        f.render_widget(
            Paragraph::new("(none)").style(Style::default().fg(theme.muted)),
            inner,
        );
        return;
    }

    let lines: Vec<Line<'static>> = r
        .labels
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(
                    k.clone(),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::raw(v.clone()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_fields(f: &mut Frame, area: Rect, r: &Record, theme: &Theme) {
    let block = section_block(" fields ", theme);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if r.fields.is_empty() {
        f.render_widget(
            Paragraph::new("(none)").style(Style::default().fg(theme.muted)),
            inner,
        );
        return;
    }

    let value = serde_json::to_value(&r.fields).unwrap_or(serde_json::Value::Null);
    let pretty = serde_json::to_string_pretty(&value).unwrap_or_default();

    let lines = syntax::highlight_json(&pretty, theme.highlighter_theme)
        .unwrap_or_else(|| jsonfmt::highlight(&value));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_raw(f: &mut Frame, area: Rect, r: &Record, theme: &Theme) {
    let block = section_block(" raw ", theme);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let text = r.raw.clone().unwrap_or_else(|| "(none)".into());
    f.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(theme.muted))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

/// Uniform rounded block helper for each detail section.
fn section_block(title: &'static str, theme: &Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(title, Style::default().fg(theme.muted)))
}

/// Centered rect covering `percent_x`/`percent_y` of the area.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, v, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);

    let [_, h, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(v);

    h
}
