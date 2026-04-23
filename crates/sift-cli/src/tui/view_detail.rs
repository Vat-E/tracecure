//! Centered 80%×80% popup over the main view. Shows full record details with
//! JSON-highlighted fields. Overlays whatever's beneath using Clear.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use sift_core::Record;

use super::jsonfmt;

pub fn draw_detail(f: &mut Frame, r: &Record) {
    let area = centered_rect(80, 80, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" record · Esc to close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [header, body_area, labels_area, fields_area, raw_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(4),
        Constraint::Length(6),
        Constraint::Min(3),
        Constraint::Length(3),
    ])
    .areas(inner);

    render_header(f, header, r);
    render_body(f, body_area, r);
    render_labels(f, labels_area, r);
    render_fields(f, fields_area, r);
    render_raw(f, raw_area, r);
}

fn render_header(f: &mut Frame, area: Rect, r: &Record) {
    let ts = r.timestamp.to_rfc3339();
    let line = Line::from(vec![
        Span::styled(ts, Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(r.host.clone(), Style::default().fg(Color::Gray)),
        Span::raw(" · "),
        Span::styled(r.app.clone(), Style::default().fg(Color::Gray)),
        Span::raw(" · "),
        Span::styled(r.stream_id.clone(), Style::default().fg(Color::Gray)),
        Span::raw(" · "),
        Span::styled(format!("{:?}", r.level), Style::default().fg(Color::Yellow)),
    ]);
    f.render_widget(Paragraph::new(line).wrap(Wrap { trim: false }), area);
}

fn render_body(f: &mut Frame, area: Rect, r: &Record) {
    let block = Block::default().title(" body ").borders(Borders::TOP);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(r.body.clone()).wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_labels(f: &mut Frame, area: Rect, r: &Record) {
    let block = Block::default().title(" labels ").borders(Borders::TOP);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if r.labels.is_empty() {
        f.render_widget(
            Paragraph::new("(none)").style(Style::default().fg(Color::DarkGray)),
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
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::raw(v.clone()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_fields(f: &mut Frame, area: Rect, r: &Record) {
    let block = Block::default().title(" fields ").borders(Borders::TOP);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if r.fields.is_empty() {
        f.render_widget(
            Paragraph::new("(none)").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }
    let value = serde_json::to_value(&r.fields).unwrap_or(serde_json::Value::Null);
    let lines = jsonfmt::highlight(&value);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_raw(f: &mut Frame, area: Rect, r: &Record) {
    let block = Block::default().title(" raw ").borders(Borders::TOP);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let text = r.raw.clone().unwrap_or_else(|| "(none)".into());
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
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
