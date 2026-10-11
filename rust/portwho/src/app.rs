//! App state and the event loop.

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use ratatui::widgets::TableState;

use crate::ports::{Collector, PortEntry};
use crate::ui;

pub struct App {
    collector: Collector,
    pub entries: Vec<PortEntry>,
    pub table: TableState,
    pub error: Option<String>,
    quit: bool,
}

impl App {
    pub fn new() -> Self {
        let mut app = Self {
            collector: Collector::new(),
            entries: Vec::new(),
            table: TableState::default(),
            error: None,
            quit: false,
        };
        app.refresh();
        app
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.quit {
            terminal.draw(|frame| ui::draw(frame, &mut self))?;
            if let Event::Key(key) = event::read()? {
                self.on_key(key);
            }
        }
        Ok(())
    }

    pub fn selected(&self) -> Option<&PortEntry> {
        self.table.selected().and_then(|i| self.entries.get(i))
    }

    pub fn refresh(&mut self) {
        // Keep the cursor on the same (port, pid) across refreshes when it still exists.
        let keep = self.selected().map(|e| (e.port, e.pid));
        match self.collector.collect() {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        let idx = keep
            .and_then(|k| self.entries.iter().position(|e| (e.port, e.pid) == k))
            .or_else(|| self.table.selected())
            .map(|i| i.min(self.entries.len().saturating_sub(1)))
            .or(Some(0))
            .filter(|_| !self.entries.is_empty());
        self.table.select(idx);
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.quit = true;
            }
            KeyCode::Down | KeyCode::Char('j') => self.table.select_next(),
            KeyCode::Up | KeyCode::Char('k') => self.table.select_previous(),
            KeyCode::Home | KeyCode::Char('g') => self.table.select_first(),
            KeyCode::End | KeyCode::Char('G') => self.table.select_last(),
            KeyCode::Char('r') => self.refresh(),
            _ => {}
        }
    }
}
