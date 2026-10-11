//! Rendering. Immediate mode: everything is redrawn from `App` each frame.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap};

use crate::app::{App, Level, Mode, Pending};
use crate::identity::shorten_path;
use crate::mem::format_bytes;
use crate::ports::{PortEntry, Scope};
use crate::view::SortKey;

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
    draw_footer(frame, app, footer);

    match &app.mode {
        Mode::Confirm(pending) => draw_confirm(frame, pending),
        Mode::Help => draw_help(frame),
        Mode::Normal | Mode::Filter => {}
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let mine = app.entries.iter().filter(|e| e.owned_by_me).count();
    let shown = if app.query.is_empty() {
        String::new()
    } else {
        format!(" · {} shown", app.visible.len())
    };
    let mut spans = vec![
        " portwho ".bold().fg(Color::Black).bg(ACCENT),
        format!("  {} listening · {mine} yours{shown}", app.entries.len()).into(),
    ];
    if let Some(err) = &app.error {
        spans.push(format!("  ⚠ {err}").red());
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn header_cell(title: &'static str, key: Option<SortKey>, app: &App) -> Cell<'static> {
    match key {
        Some(k) if k == app.sort => {
            // ▼ = the key's natural "most interesting first" order; ▲ when reversed.
            let arrow = if app.reverse { "▲" } else { "▼" };
            Cell::from(format!("{title}{arrow}")).underlined()
        }
        _ => Cell::from(title),
    }
}

fn draw_table(frame: &mut Frame, app: &mut App, area: Rect) {
    let header = Row::new([
        header_cell("PORT", Some(SortKey::Port), app),
        header_cell("BIND", None, app),
        header_cell("PROJECT", Some(SortKey::Project), app),
        header_cell("COMMAND", None, app),
        header_cell("PID", None, app),
        header_cell("CPU", Some(SortKey::Cpu), app),
        header_cell("MEM", Some(SortKey::Memory), app),
        header_cell("UP", Some(SortKey::Uptime), app),
    ])
    .style(Style::new().bold().fg(ACCENT));

    let rows = app.visible.iter().map(|&i| row(&app.entries[i]));
    let widths = [
        Constraint::Length(6),
        Constraint::Length(16),
        Constraint::Fill(2),
        Constraint::Fill(3),
        Constraint::Length(7),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Length(7),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::new().borders(Borders::TOP | Borders::BOTTOM))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(table, area, &mut app.table);

    if app.visible.is_empty() && app.error.is_none() {
        let msg = if app.query.is_empty() {
            "No listening TCP ports found.".to_string()
        } else {
            format!("Nothing matches “{}”. Esc clears the filter.", app.query)
        };
        frame.render_widget(
            Paragraph::new(msg).dark_gray(),
            area.inner(Margin::new(2, 2)),
        );
    }
}

fn row(e: &PortEntry) -> Row<'static> {
    let bind = e.bind();
    let bind_style = match bind.scope {
        Scope::All => Style::new().fg(Color::Yellow),
        Scope::Localhost => Style::new().fg(Color::Green),
        Scope::Specific => Style::new(),
    };
    let project = Cell::from(e.label());
    let project = if e.forwarder.is_some() {
        project.magenta()
    } else {
        project
    };
    let row = Row::new([
        Cell::from(e.port.to_string()).bold(),
        Cell::from(bind.label()).style(bind_style),
        project,
        Cell::from(e.short_cmd()),
        Cell::from(e.pid.to_string()),
        Cell::from(format!("{:.1}", e.cpu)),
        Cell::from(e.mem_bytes.map(format_bytes).unwrap_or_default()),
        Cell::from(e.uptime_secs.map(format_uptime).unwrap_or_default()),
    ]);
    if e.owned_by_me { row } else { row.dark_gray() }
}

fn draw_detail(frame: &mut Frame, app: &App, area: Rect) {
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

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let line = if matches!(app.mode, Mode::Filter) {
        Line::from(vec![
            " / ".bold().fg(Color::Black).bg(ACCENT),
            format!(" {}", app.query).into(),
            "▏".fg(ACCENT),
            "   enter keep · esc clear".dark_gray(),
        ])
    } else if let Some(status) = &app.status {
        let style = match status.level {
            Level::Info => Style::new().fg(ACCENT),
            Level::Ok => Style::new().fg(Color::Green).bold(),
            Level::Warn => Style::new().fg(Color::Yellow).bold(),
        };
        Line::from(Span::styled(format!(" {}", status.text), style))
    } else {
        let mut spans = hints(&[
            ("↑↓", "move"),
            ("/", "filter"),
            ("s", "sort"),
            ("x", "kill"),
            ("?", "help"),
            ("q", "quit"),
        ]);
        if !app.query.is_empty() {
            spans.push(format!(" filter: {} ", app.query).fg(ACCENT).italic());
        }
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_confirm(frame: &mut Frame, p: &Pending) {
    let mut lines = vec![
        Line::from(vec![
            "Free ".into(),
            format!(":{}", p.port).bold(),
            " by stopping ".into(),
            p.label.clone().bold(),
            format!(" ({})", p.cmd).dark_gray(),
            "?".into(),
        ]),
        Line::default(),
        Line::from(vec![
            "tree  ".fg(ACCENT),
            p.plan.chain.join(" → ").into(),
            format!(
                "  ({} process{})",
                p.plan.pids.len(),
                if p.plan.pids.len() == 1 { "" } else { "es" }
            )
            .dark_gray(),
        ]),
    ];
    if !p.also_frees.is_empty() {
        let ports: Vec<String> = p.also_frees.iter().map(|port| format!(":{port}")).collect();
        lines.push(Line::from(vec![
            "also  ".fg(ACCENT),
            format!("frees {}", ports.join(" ")).into(),
        ]));
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![
        " y ".bold().fg(Color::Black).bg(Color::Red),
        " stop it   ".into(),
        " n ".bold().fg(Color::Black).bg(Color::Gray),
        " cancel".into(),
    ]));
    let height = u16::try_from(lines.len()).unwrap_or(8) + 2;
    let area = centered(frame.area(), 72, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .title(" stop process ")
                .border_style(Style::new().fg(Color::Red))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        ),
        area,
    );
}

const HELP: &[(&str, &str)] = &[
    ("↑↓ / j k", "move · PgUp PgDn · g G top/bottom"),
    ("/", "filter: words, or :30 for ports starting with 30"),
    ("esc", "clear the filter"),
    ("s / S", "next sort column / reverse order"),
    ("x / del", "stop the process tree and free its port"),
    ("r", "refresh now"),
    ("q / ctrl-c", "quit"),
];

fn draw_help(frame: &mut Frame) {
    let mut lines: Vec<Line> = HELP
        .iter()
        .map(|(k, v)| Line::from(vec![format!("{k:<12}").bold().fg(ACCENT), (*v).into()]))
        .collect();
    lines.push(Line::default());
    lines.push(Line::from(vec![
        "bind  ".dark_gray(),
        "* ".yellow(),
        "= open to your network   ".dark_gray(),
        "localhost".green(),
        " = this Mac only".dark_gray(),
    ]));
    lines.push(Line::from(
        "greyed rows belong to another user or root".dark_gray(),
    ));
    let height = u16::try_from(lines.len()).unwrap_or(10) + 2;
    let area = centered(frame.area(), 70, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title(" keys · any key closes ")
                .border_style(ACCENT)
                .padding(ratatui::widgets::Padding::horizontal(1)),
        ),
        area,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [v] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [h] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(v);
    h
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
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(42), "42s");
        assert_eq!(format_uptime(12 * 60 + 5), "12m");
        assert_eq!(format_uptime(3 * 3600 + 5 * 60), "3h 5m");
        assert_eq!(format_uptime(2 * 86_400 + 4 * 3600), "2d 4h");
    }
}
