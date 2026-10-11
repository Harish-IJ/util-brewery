//! The interactive table: key handling and what ends up on screen.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portwho::app::{App, Level, Mode, Pending};
use portwho::kill::Plan;
use portwho::ports::PortEntry;
use portwho::ui;
use portwho::view::SortKey;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn entry(port: u16, pid: u32, project: &str, cmd: &str, cpu: f32, mine: bool) -> PortEntry {
    PortEntry {
        port,
        pid,
        name: "node".into(),
        binds: vec!["127.0.0.1".parse().unwrap()],
        project: Some(project.into()),
        cmd: cmd.split(' ').map(str::to_owned).collect(),
        cpu,
        mem_bytes: Some(50 * 1024 * 1024),
        uptime_secs: Some(3 * 3600),
        owned_by_me: mine,
        ..Default::default()
    }
}

fn app() -> App {
    App::with_entries(vec![
        entry(
            5173,
            30,
            "acme-web",
            "node /w/node_modules/.bin/vite",
            2.0,
            true,
        ),
        entry(3000, 20, "acme-api", "node server.js", 7.5, true),
        entry(5432, 10, "postgres", "postgres -D /var/db", 0.1, false),
    ])
}

fn press(app: &mut App, keys: &str) {
    for c in keys.chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
}

fn key(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn selected_port(app: &App) -> Option<u16> {
    app.selected().map(|e| e.port)
}

fn visible_ports(app: &App) -> Vec<u16> {
    app.visible.iter().map(|&i| app.entries[i].port).collect()
}

fn render(app: &mut App) -> String {
    let mut term = Terminal::new(TestBackend::new(110, 20)).unwrap();
    term.draw(|f| ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    buf.content
        .chunks(buf.area.width as usize)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn starts_sorted_by_port_with_first_row_selected() {
    let app = app();
    assert_eq!(visible_ports(&app), [3000, 5173, 5432]);
    assert_eq!(selected_port(&app), Some(3000));
    assert!(matches!(app.mode, Mode::Normal));
}

#[test]
fn navigation_clamps_at_both_ends() {
    let mut app = app();
    press(&mut app, "k");
    assert_eq!(selected_port(&app), Some(3000));
    press(&mut app, "jjjj");
    assert_eq!(selected_port(&app), Some(5432));
    press(&mut app, "g");
    assert_eq!(selected_port(&app), Some(3000));
    press(&mut app, "G");
    assert_eq!(selected_port(&app), Some(5432));
    key(&mut app, KeyCode::PageUp);
    assert_eq!(selected_port(&app), Some(3000));
}

#[test]
fn sorting_keeps_the_cursor_on_the_same_row() {
    let mut app = app();
    press(&mut app, "j"); // 5173
    press(&mut app, "s"); // project
    assert_eq!(app.sort, SortKey::Project);
    assert_eq!(visible_ports(&app), [3000, 5173, 5432]);
    press(&mut app, "s"); // cpu, busiest first
    assert_eq!(visible_ports(&app), [3000, 5173, 5432]);
    press(&mut app, "S");
    assert!(app.reverse);
    assert_eq!(visible_ports(&app), [5432, 5173, 3000]);
    assert_eq!(selected_port(&app), Some(5173));
    press(&mut app, "s"); // a new key resets the direction
    assert_eq!((app.sort, app.reverse), (SortKey::Memory, false));
}

#[test]
fn filter_mode_edits_live_and_esc_clears() {
    let mut app = app();
    press(&mut app, "/acme");
    assert!(matches!(app.mode, Mode::Filter));
    assert_eq!(visible_ports(&app), [3000, 5173]);
    press(&mut app, " vite");
    assert_eq!(visible_ports(&app), [5173]);
    assert_eq!(selected_port(&app), Some(5173));
    key(&mut app, KeyCode::Backspace);
    assert_eq!(app.query, "acme vit");
    press(&mut app, "q"); // typed, not quit
    assert!(!app.quit);
    assert_eq!(app.query, "acme vitq");
    assert!(visible_ports(&app).is_empty());
    assert_eq!(selected_port(&app), None);

    key(&mut app, KeyCode::Esc);
    assert!(matches!(app.mode, Mode::Normal));
    assert!(app.query.is_empty());
    assert_eq!(visible_ports(&app), [3000, 5173, 5432]);
}

#[test]
fn enter_keeps_the_filter_and_esc_in_normal_mode_clears_it() {
    let mut app = app();
    press(&mut app, "/:54");
    key(&mut app, KeyCode::Enter);
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(visible_ports(&app), [5432]);
    key(&mut app, KeyCode::Esc);
    assert_eq!(visible_ports(&app), [3000, 5173, 5432]);
    assert!(!app.quit, "esc never quits");
}

#[test]
fn killing_another_users_process_is_refused_up_front() {
    let mut app = app();
    press(&mut app, "G"); // postgres, not ours
    press(&mut app, "x");
    assert!(matches!(app.mode, Mode::Normal));
    let status = app.status.as_ref().unwrap();
    assert_eq!(status.level, Level::Warn);
    assert!(
        status.text.contains(":5432 owned by another user"),
        "{}",
        status.text
    );
}

#[test]
fn quit_keys() {
    let mut app = app();
    press(&mut app, "q");
    assert!(app.quit);

    let mut app = self::app();
    press(&mut app, "/");
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.quit, "ctrl-c quits even while filtering");
}

#[test]
fn renders_table_detail_and_hints() {
    let mut app = app();
    let screen = render(&mut app);
    for needle in [
        "portwho",
        "3 listening · 2 yours",
        "PORT▼",
        "acme-api",
        "vite",
        "localhost v4",
        "7.5",
        "50.0M",
        "3h 0m",
        "cmd node server.js",
        "x kill",
    ] {
        assert!(screen.contains(needle), "{needle:?} missing:\n{screen}");
    }
    let first_row = screen.lines().find(|l| l.contains("3000")).unwrap();
    assert!(
        first_row.starts_with("▶"),
        "selected row is marked:\n{screen}"
    );
}

#[test]
fn renders_empty_filter_message_and_query() {
    let mut app = app();
    press(&mut app, "/nope");
    let screen = render(&mut app);
    assert!(screen.contains("Nothing matches “nope”"), "{screen}");
    assert!(screen.contains("/  nope"), "{screen}");
    assert!(screen.contains("0 shown"), "{screen}");
}

#[test]
fn renders_kill_confirmation_and_cancels() {
    let mut app = app();
    app.mode = Mode::Confirm(Box::new(Pending {
        plan: Plan {
            target: 30,
            root: 28,
            pids: vec![30, 29, 28],
            chain: vec!["npm".into(), "sh".into(), "vite".into()],
        },
        port: 5173,
        label: "acme-web".into(),
        cmd: "vite".into(),
        also_frees: vec![24678],
    }));
    let screen = render(&mut app);
    for needle in [
        "Free :5173 by stopping acme-web (vite)?",
        "npm → sh → vite  (3 processes)",
        "frees :24678",
        "y  stop it",
    ] {
        assert!(screen.contains(needle), "{needle:?} missing:\n{screen}");
    }
    press(&mut app, "n");
    assert!(matches!(app.mode, Mode::Normal));
    assert!(!app.is_killing(), "cancel starts nothing");
}

#[test]
fn help_overlay_opens_and_any_key_closes() {
    let mut app = app();
    press(&mut app, "?");
    assert!(render(&mut app).contains("any key closes"));
    press(&mut app, "j");
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(
        selected_port(&app),
        Some(3000),
        "closing key isn't also a move"
    );
}
