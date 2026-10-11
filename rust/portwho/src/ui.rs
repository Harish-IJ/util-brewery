//! Rendering. Immediate mode: everything is redrawn from `App` each frame.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::app::App;
use crate::identity::shorten_path;
use crate::ports::PortEntry;

const ACCENT: Color = Color::Cyan;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [header, body, detail, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(5),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_header(frame, app, header);
    draw_table(frame, app, body);
    draw_detail(frame, app, detail);
    frame.render_widget(
        Paragraph::new(Line::from(hints(&[
            ("↑↓/jk", "move"),
            ("g/G", "top/bottom"),
            ("r", "refresh"),
            ("q", "quit"),
        ]))),
        footer,
    );
}

fn draw_header(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut spans = vec![
        " portwho ".bold().fg(Color::Black).bg(ACCENT),
        format!("  {} listening", app.entries.len()).into(),
    ];
    if let Some(err) = &app.error {
        spans.push(format!("  ⚠ {err}").red());
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_table(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let header = Row::new(["PORT", "BIND", "PROJECT", "COMMAND", "PID", "UP"])
        .style(Style::new().bold().fg(ACCENT));

    let rows = app.entries.iter().map(|e| {
        let row = Row::new([
            Cell::from(e.port.to_string()).bold(),
            Cell::from(e.bind_label()),
            Cell::from(e.label()),
            Cell::from(e.short_cmd()),
            Cell::from(e.pid.to_string()),
            Cell::from(e.uptime_secs.map(format_uptime).unwrap_or_default()),
        ]);
        if e.owned_by_me { row } else { row.dark_gray() }
    });

    let widths = [
        Constraint::Length(6),
        Constraint::Length(16),
        Constraint::Fill(2),
        Constraint::Fill(3),
        Constraint::Length(7),
        Constraint::Length(7),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::new().borders(Borders::TOP | Borders::BOTTOM))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(table, area, &mut app.table);

    if app.entries.is_empty() && app.error.is_none() {
        let msg = Paragraph::new("No listening TCP ports found.").dark_gray();
        frame.render_widget(msg, area.inner(ratatui::layout::Margin::new(2, 2)));
    }
}

fn draw_detail(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let Some(e) = app.selected() else {
        return;
    };
    let lines = vec![
        field("cwd ", e.cwd.as_deref().map(shorten_path), e),
        field("cmd ", (!e.cmd.is_empty()).then(|| e.cmd.join(" ")), e),
        field("exe ", e.exe.as_deref().map(shorten_path), e),
    ];
    let title = format!(" {} · pid {} ", e.name, e.pid);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(title).border_style(ACCENT)),
        area,
    );
}

fn field(label: &'static str, value: Option<String>, e: &PortEntry) -> Line<'static> {
    let value = match value {
        Some(v) => Span::raw(v),
        None if !e.owned_by_me => "owned by another user (run with sudo for details)"
            .dark_gray()
            .italic(),
        None => "—".dark_gray(),
    };
    Line::from(vec![label.fg(ACCENT), value])
}

fn hints(pairs: &[(&'static str, &'static str)]) -> Vec<Span<'static>> {
    pairs
        .iter()
        .flat_map(|(key, what)| {
            [
                format!(" {key} ").bold().fg(ACCENT),
                format!("{what} ").into(),
            ]
        })
        .collect()
}

/// 42s, 12m, 3h 5m, 2d 4h.
pub fn format_uptime(secs: u64) -> String {
    let (m, h, d) = (secs / 60, secs / 3600, secs / 86_400);
    match () {
        () if d > 0 => format!("{d}d {}h", h % 24),
        () if h > 0 => format!("{h}h {}m", m % 60),
        () if m > 0 => format!("{m}m"),
        () => format!("{secs}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::format_uptime;

    #[test]
    fn uptime() {
        assert_eq!(format_uptime(42), "42s");
        assert_eq!(format_uptime(12 * 60 + 5), "12m");
        assert_eq!(format_uptime(3 * 3600 + 5 * 60), "3h 5m");
        assert_eq!(format_uptime(2 * 86_400 + 4 * 3600), "2d 4h");
    }
}
