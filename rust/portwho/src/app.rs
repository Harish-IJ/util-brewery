//! App state and the event loop.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use ratatui::widgets::TableState;

use crate::kill::{self, KillJob, Plan, Progress};
use crate::ports::{Collector, PortEntry};
use crate::ui;
use crate::view::{self, SortKey};

const STATUS_TTL: Duration = Duration::from_secs(6);
const JOB_TICK: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub enum Mode {
    Normal,
    /// Typing into the `/` filter.
    Filter,
    Confirm(Box<Pending>),
    Help,
}

/// A kill waiting for `y`.
#[derive(Debug)]
pub struct Pending {
    pub plan: Plan,
    pub port: u16,
    pub label: String,
    pub cmd: String,
    /// Other ports held by processes in the plan.
    pub also_frees: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Ok,
    Warn,
}

#[derive(Debug)]
pub struct Status {
    pub text: String,
    pub level: Level,
    at: Instant,
}

struct Job {
    job: KillJob,
    port: u16,
}

pub struct App {
    /// `None` in tests: the app then works on whatever entries it was given.
    collector: Option<Collector>,
    pub entries: Vec<PortEntry>,
    /// Indices into `entries`, filtered and sorted.
    pub visible: Vec<usize>,
    pub table: TableState,
    pub query: String,
    pub mode: Mode,
    pub sort: SortKey,
    pub reverse: bool,
    pub status: Option<Status>,
    pub error: Option<String>,
    pub interval: Duration,
    pub grace: Duration,
    job: Option<Job>,
    last_refresh: Instant,
    pub quit: bool,
}

impl App {
    pub fn new(interval: Duration) -> Self {
        let mut app = Self::with_entries(Vec::new());
        app.collector = Some(Collector::new());
        app.interval = interval;
        app.refresh();
        app
    }

    /// An app over a fixed list of entries, with no live collection.
    pub fn with_entries(entries: Vec<PortEntry>) -> Self {
        let mut app = Self {
            collector: None,
            entries,
            visible: Vec::new(),
            table: TableState::default(),
            query: String::new(),
            mode: Mode::Normal,
            sort: SortKey::default(),
            reverse: false,
            status: None,
            error: None,
            interval: Duration::from_secs(2),
            grace: kill::DEFAULT_GRACE,
            job: None,
            last_refresh: Instant::now(),
            quit: false,
        };
        app.recompute();
        app
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.quit {
            terminal.draw(|frame| ui::draw(frame, &mut self))?;
            if event::poll(self.next_wake())?
                && let Event::Key(key) = event::read()?
            {
                self.on_key(key);
            }
            self.tick();
        }
        Ok(())
    }

    /// How long the loop may sleep. Idle cost is one collection per `interval`.
    fn next_wake(&self) -> Duration {
        if self.job.is_some() {
            return JOB_TICK;
        }
        let mut wake = self.interval.saturating_sub(self.last_refresh.elapsed());
        if let Some(s) = &self.status {
            wake = wake.min(STATUS_TTL.saturating_sub(s.at.elapsed()));
        }
        wake
    }

    fn tick(&mut self) {
        if let Some(job) = &mut self.job {
            match job.job.poll() {
                Progress::Waiting => {}
                done => {
                    let port = job.port;
                    self.job = None;
                    self.refresh();
                    self.report(port, &done);
                }
            }
        }
        if self
            .status
            .as_ref()
            .is_some_and(|s| s.at.elapsed() >= STATUS_TTL)
        {
            self.status = None;
        }
        if self.last_refresh.elapsed() >= self.interval {
            self.refresh();
        }
    }

    pub fn selected(&self) -> Option<&PortEntry> {
        let i = *self.visible.get(self.table.selected()?)?;
        self.entries.get(i)
    }

    pub fn is_killing(&self) -> bool {
        self.job.is_some()
    }

    pub fn refresh(&mut self) {
        self.last_refresh = Instant::now();
        let Some(collector) = &mut self.collector else {
            return;
        };
        match collector.collect() {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        self.recompute();
    }

    /// Re-filters and re-sorts, keeping the cursor on the same row when it's still shown.
    fn recompute(&mut self) {
        let keep = self.selected().map(|e| (e.port, e.pid));
        let prev = self.table.selected().unwrap_or(0);
        self.visible = view::visible(&self.entries, &self.query, self.sort, self.reverse);
        let idx = keep
            .and_then(|k| {
                self.visible
                    .iter()
                    .position(|&i| (self.entries[i].port, self.entries[i].pid) == k)
            })
            .unwrap_or(prev);
        self.table.select(if self.visible.is_empty() {
            None
        } else {
            Some(idx.min(self.visible.len() - 1))
        });
    }

    fn move_by(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() - 1;
        let cur = self.table.selected().unwrap_or(0);
        self.table
            .select(Some(cur.saturating_add_signed(delta).min(last)));
    }

    fn set_status(&mut self, level: Level, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            level,
            at: Instant::now(),
        });
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        match &self.mode {
            Mode::Normal => self.on_normal_key(key),
            Mode::Filter => self.on_filter_key(key),
            Mode::Confirm(_) => match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => self.confirm_kill(),
                KeyCode::Char('n' | 'N' | 'q') | KeyCode::Esc => self.mode = Mode::Normal,
                _ => {}
            },
            Mode::Help => self.mode = Mode::Normal,
        }
    }

    fn on_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc if !self.query.is_empty() => {
                self.query.clear();
                self.recompute();
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::Home | KeyCode::Char('g') => self.move_by(isize::MIN),
            KeyCode::End | KeyCode::Char('G') => self.move_by(isize::MAX),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('s') => {
                self.sort = self.sort.next();
                self.reverse = false;
                self.recompute();
            }
            KeyCode::Char('S') => {
                self.reverse = !self.reverse;
                self.recompute();
            }
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('x') | KeyCode::Delete => self.request_kill(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
    }

    fn on_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.mode = Mode::Normal,
            KeyCode::Esc => {
                self.query.clear();
                self.mode = Mode::Normal;
            }
            KeyCode::Down => self.move_by(1),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Backspace => {
                self.query.pop();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.clear();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.push(c);
            }
            _ => return,
        }
        self.recompute();
    }

    fn request_kill(&mut self) {
        if self.job.is_some() {
            self.set_status(Level::Warn, "already stopping a process, hang on");
            return;
        }
        let Some(e) = self.selected().cloned() else {
            return;
        };
        if !e.owned_by_me {
            self.set_status(
                Level::Warn,
                format!(":{} {}", e.port, kill::Refusal::NotMine),
            );
            return;
        }
        let table = kill::snapshot();
        match kill::plan(
            e.pid,
            &table,
            crate::ports::current_uid(),
            std::process::id(),
        ) {
            Ok(plan) => {
                let mut also_frees: Vec<u16> = self
                    .entries
                    .iter()
                    .filter(|o| o.port != e.port && plan.pids.contains(&o.pid))
                    .map(|o| o.port)
                    .collect();
                also_frees.sort_unstable();
                also_frees.dedup();
                self.mode = Mode::Confirm(Box::new(Pending {
                    plan,
                    port: e.port,
                    label: e.label(),
                    cmd: e.short_cmd(),
                    also_frees,
                }));
            }
            Err(refusal) => self.set_status(Level::Warn, format!(":{} {refusal}", e.port)),
        }
    }

    fn confirm_kill(&mut self) {
        let Mode::Confirm(pending) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        self.set_status(Level::Info, format!("stopping :{}…", pending.port));
        self.job = Some(Job {
            job: KillJob::start(&pending.plan, self.grace),
            port: pending.port,
        });
    }

    /// After a kill, say plainly whether the port is actually free now.
    fn report(&mut self, port: u16, progress: &Progress) {
        let holder = self.entries.iter().find(|e| e.port == port);
        let (level, text) = match (progress, holder) {
            (Progress::Stuck { survivors }, _) => (
                Level::Warn,
                format!("⚠ :{port}: pids {survivors:?} survived SIGKILL"),
            ),
            (_, Some(h)) => (
                Level::Warn,
                format!(
                    "⚠ :{port} is held again by pid {} ({}); a supervisor may have restarted it",
                    h.pid,
                    h.label()
                ),
            ),
            (Progress::Done { killed, forced }, None) => {
                let forced = if *forced > 0 {
                    format!(", {forced} forced")
                } else {
                    String::new()
                };
                let plural = if *killed == 1 { "" } else { "es" };
                (
                    Level::Ok,
                    format!("✓ :{port} is free (stopped {killed} process{plural}{forced})"),
                )
            }
            (Progress::Waiting, None) => return,
        };
        self.set_status(level, text);
    }
}
