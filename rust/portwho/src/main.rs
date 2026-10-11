use std::process::ExitCode;
use std::time::Duration;

use portwho::{app, mem, ports, ui};

const HELP: &str = "\
portwho: see which process owns each listening port, and free it

USAGE:
    portwho [OPTIONS]

OPTIONS:
    -l, --list             print the table once and exit
    -i, --interval <SECS>  refresh interval for the live table [default: 2]
    -h, --help             print this help
    -V, --version          print the version

Press ? inside the table for keys.
";

enum Cmd {
    Tui(Duration),
    List,
    Help,
    Version,
}

fn parse(args: &[String]) -> Result<Cmd, String> {
    let mut interval = Duration::from_secs(2);
    let mut list = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-l" | "--list" => list = true,
            "-h" | "--help" => return Ok(Cmd::Help),
            "-V" | "--version" => return Ok(Cmd::Version),
            "-i" | "--interval" => {
                let value = it.next().ok_or("--interval needs a value")?;
                let secs: f64 = value
                    .parse()
                    .ok()
                    .filter(|s: &f64| s.is_finite() && *s >= 0.5 && *s <= 3600.0)
                    .ok_or_else(|| format!("invalid interval `{value}` (0.5 to 3600 seconds)"))?;
                interval = Duration::from_secs_f64(secs);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(if list { Cmd::List } else { Cmd::Tui(interval) })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Cmd::Tui(interval)) => {
            match ratatui::run(|terminal| app::App::new(interval).run(terminal)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(&e.to_string()),
            }
        }
        Ok(Cmd::List) => print_list(),
        Ok(Cmd::Help) => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Cmd::Version) => {
            println!("portwho {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("{e}\n\n{HELP}")),
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("portwho: {msg}");
    ExitCode::FAILURE
}

fn print_list() -> ExitCode {
    let entries = match ports::Collector::new().collect() {
        Ok(entries) => entries,
        Err(e) => return fail(&e),
    };
    println!(
        "{:<6} {:<16} {:<28} {:<36} {:>7} {:>7} {:>7}",
        "PORT", "BIND", "PROJECT", "COMMAND", "PID", "MEM", "UP"
    );
    for e in &entries {
        println!(
            "{:<6} {:<16} {:<28} {:<36} {:>7} {:>7} {:>7}",
            e.port,
            e.bind().label(),
            truncate(&e.label(), 28),
            truncate(&e.short_cmd(), 36),
            e.pid,
            e.mem_bytes.map(mem::format_bytes).unwrap_or_default(),
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
