//! portwho: a live "who owns this port?" table for macOS dev machines.

mod app;
mod identity;
mod ports;
mod ui;

use std::process::ExitCode;

const HELP: &str = "\
portwho: see which process owns each listening port

USAGE:
    portwho           open the live table
    portwho --list    print the table once and exit
";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => match ratatui::run(|terminal| app::App::new().run(terminal)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("portwho: {e}");
                ExitCode::FAILURE
            }
        },
        Some("--list" | "-l") => print_list(),
        Some("--help" | "-h") => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("portwho: unknown argument `{other}`\n\n{HELP}");
            ExitCode::FAILURE
        }
    }
}

fn print_list() -> ExitCode {
    let entries = match ports::Collector::new().collect() {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("portwho: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "{:<6} {:<16} {:<28} {:<36} {:>7} {:>7}",
        "PORT", "BIND", "PROJECT", "COMMAND", "PID", "UP"
    );
    for e in &entries {
        println!(
            "{:<6} {:<16} {:<28} {:<36} {:>7} {:>7}",
            e.port,
            e.bind_label(),
            truncate(&e.label(), 28),
            truncate(&e.short_cmd(), 36),
            e.pid,
            e.uptime_secs.map(ui::format_uptime).unwrap_or_default(),
        );
    }
    ExitCode::SUCCESS
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}
