//! Against real processes: discovery, identification, and freeing a port.

mod common;

use std::time::Duration;

use common::{Fixture, port_is_listening};
use portwho::kill::{self, KillJob, Progress, Refusal};
use portwho::ports::{Collector, Scope, current_uid};

#[test]
fn identifies_listener_by_project_and_command() {
    let fx = Fixture::start("e2e-identify", false);
    let entries = Collector::new().collect().unwrap();
    let e = entries
        .iter()
        .find(|e| e.port == fx.port)
        .expect("fixture port should be listed");

    assert_eq!(e.pid, fx.server_pid);
    assert_eq!(e.label(), "e2e-identify");
    // `python3`, `python3.12` or macOS's `Python`, always with the inline-code flag.
    let cmd = e.short_cmd();
    assert!(
        cmd.to_lowercase().starts_with("python") && cmd.ends_with(" -c"),
        "{cmd}"
    );
    assert_eq!(e.bind().scope, Scope::Localhost);
    assert!(e.owned_by_me);
    assert!(e.mem_bytes.is_some_and(|b| b > 0));
    assert!(e.uptime_secs.is_some());
}

#[test]
fn kill_frees_port_and_takes_the_wrapper_shell() {
    let fx = Fixture::start("e2e-kill", false);
    let plan = kill::plan(
        fx.server_pid,
        &kill::snapshot(),
        current_uid(),
        std::process::id(),
    )
    .unwrap();
    assert_eq!(plan.root, fx.shell_pid);
    assert_eq!(plan.pids, [fx.server_pid, fx.shell_pid]);

    let outcome = KillJob::start(&plan, kill::DEFAULT_GRACE).wait();
    assert_eq!(
        outcome,
        Progress::Done {
            killed: 2,
            forced: 0
        }
    );
    assert!(!port_is_listening(fx.port));
}

#[test]
fn escalates_to_sigkill_when_sigterm_is_ignored() {
    let fx = Fixture::start("e2e-stubborn", true);
    let plan = kill::plan(
        fx.server_pid,
        &kill::snapshot(),
        current_uid(),
        std::process::id(),
    )
    .unwrap();

    let outcome = KillJob::start(&plan, Duration::from_millis(300)).wait();
    assert_eq!(
        outcome,
        Progress::Done {
            killed: 2,
            forced: 1
        },
        "sh obeys SIGTERM, python needs SIGKILL"
    );
    assert!(!port_is_listening(fx.port));
}

#[test]
fn never_plans_to_kill_itself_or_its_ancestors() {
    let table = kill::snapshot();
    let me = std::process::id();
    assert_eq!(
        kill::plan(me, &table, current_uid(), me),
        Err(Refusal::OwnAncestor)
    );
    if let Some(parent) = table[&me].ppid.filter(|p| *p > 1) {
        let refusal = kill::plan(parent, &table, current_uid(), me).unwrap_err();
        assert!(
            matches!(refusal, Refusal::OwnAncestor | Refusal::NotMine),
            "{refusal:?}"
        );
    }
}
