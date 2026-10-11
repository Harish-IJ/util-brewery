//! The `portwho` binary's non-interactive surface.

mod common;

use std::process::{Command, Output};

fn portwho(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_portwho"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn list_shows_the_fixture_row() {
    let fx = common::Fixture::start("cli-list", false);
    let out = portwho(&["--list"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("PORT "), "{text}");
    let row = text
        .lines()
        .find(|l| l.starts_with(&format!("{} ", fx.port)))
        .unwrap_or_else(|| panic!("no row for :{} in\n{text}", fx.port));
    for needle in [
        "localhost v4",
        "cli-list",
        " -c ",
        &fx.server_pid.to_string(),
    ] {
        assert!(row.contains(needle), "{needle:?} missing from {row:?}");
    }
}

#[test]
fn help_and_version() {
    let help = portwho(&["--help"]);
    assert!(help.status.success());
    assert!(stdout(&help).contains("--interval <SECS>"));

    let version = portwho(&["-V"]);
    assert!(version.status.success());
    assert_eq!(
        stdout(&version).trim(),
        format!("portwho {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn rejects_bad_arguments() {
    for (args, msg) in [
        (&["--bogus"][..], "unknown argument `--bogus`"),
        (&["--interval"], "--interval needs a value"),
        (&["-i", "0.1"], "invalid interval `0.1`"),
        (&["-i", "abc"], "invalid interval `abc`"),
        (&["-i", "inf"], "invalid interval `inf`"),
    ] {
        let out = portwho(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(stderr(&out).contains(msg), "{args:?}: {}", stderr(&out));
        assert!(stdout(&out).is_empty());
    }
}
