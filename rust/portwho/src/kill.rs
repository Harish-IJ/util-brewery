//! Freeing a port: decide *what* to kill (the plan), then do it (the job).
//!
//! Killing only the listener often isn't enough: `npm run dev` → `sh -c vite` →
//! `node vite` leaves npm behind, and `nodemon` would just restart it. So the plan
//! climbs from the listener through single-purpose wrappers (npm, sh -c, nodemon…)
//! and kills that whole subtree. It stops at shells, terminals and multi-process
//! runners (concurrently, turbo…) so sibling servers survive.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

use crate::identity;

/// Launchers that exist only to run the server below them.
const WRAPPERS: &[&str] = &[
    "npm",
    "npx",
    "pnpm",
    "pnpx",
    "yarn",
    "bun",
    "bunx",
    "node",
    "deno",
    "nodemon",
    "tsx",
    "ts-node",
    "ts-node-dev",
    "vite-node",
    "python",
    "python3",
    "uv",
    "poetry",
    "pipenv",
    "ruby",
    "bundle",
    "cargo",
    "cargo-watch",
    "go",
    "air",
    "watchexec",
];

/// Runners that manage several services; killing one would take its siblings down.
const MULTI_RUNNERS: &[&str] = &[
    "concurrently",
    "turbo",
    "nx",
    "npm-run-all",
    "run-p",
    "run-s",
    "lerna",
    "foreman",
    "overmind",
    "honcho",
    "pm2",
    "docker",
    "docker-compose",
    "tmux",
    "screen",
];

const SHELLS: &[&str] = &["sh", "bash", "zsh", "fish", "dash", "ksh"];

/// What `plan` needs to know about a process.
#[derive(Debug, Clone, Default)]
pub struct ProcInfo {
    pub ppid: Option<u32>,
    pub name: String,
    pub cmd: Vec<String>,
    pub uid: Option<u32>,
}

pub type ProcTable = HashMap<u32, ProcInfo>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Gone,
    NotMine,
    Protected,
    /// Killing it would kill portwho itself (e.g. the terminal it runs in).
    OwnAncestor,
    Container(&'static str),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gone => write!(f, "process already exited"),
            Self::NotMine => write!(f, "owned by another user, run portwho with sudo to kill it"),
            Self::Protected => write!(f, "refusing to kill a system process"),
            Self::OwnAncestor => write!(f, "that would kill portwho's own terminal"),
            Self::Container(rt) => {
                write!(
                    f,
                    "forwarded by {rt}; stop the container instead (e.g. `docker ps`)"
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub target: u32,
    /// Topmost process to kill; the target or one of its wrapper ancestors.
    pub root: u32,
    /// `root` and every descendant we own, deepest first.
    pub pids: Vec<u32>,
    /// Program names from `root` down to `target`, for the confirm dialog.
    pub chain: Vec<String>,
}

pub fn plan(target: u32, table: &ProcTable, my_uid: u32, my_pid: u32) -> Result<Plan, Refusal> {
    let info = table.get(&target).ok_or(Refusal::Gone)?;
    if target <= 1 {
        return Err(Refusal::Protected);
    }
    if info.uid != Some(my_uid) {
        return Err(Refusal::NotMine);
    }
    if let Some(rt) = identity::forwarder(&info.name, None, &info.cmd) {
        return Err(Refusal::Container(rt));
    }

    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, p) in table {
        if let Some(ppid) = p.ppid {
            children.entry(ppid).or_default().push(*pid);
        }
    }

    // Climb only through wrappers whose sole child is the current process, so a
    // launcher that started several servers (`sh -c "api & web"`) is left alone.
    let mut root = target;
    let mut chain = vec![identity::program(&info.cmd, &info.name)];
    while let Some(parent) = table[&root].ppid.filter(|p| *p > 1) {
        let Some(p) = table.get(&parent) else { break };
        let only_child = children.get(&parent).is_some_and(|kids| kids == &[root]);
        if p.uid != Some(my_uid) || !only_child || !is_wrapper(p) {
            break;
        }
        root = parent;
        chain.insert(0, identity::program(&p.cmd, &p.name));
    }

    let mut pids = Vec::new();
    collect_subtree(root, &children, table, my_uid, &mut pids);

    if pids.contains(&my_pid) || is_ancestor(target, my_pid, table) {
        return Err(Refusal::OwnAncestor);
    }
    Ok(Plan {
        target,
        root,
        pids,
        chain,
    })
}

/// Post-order walk so children are signalled before the parents that might respawn them.
fn collect_subtree(
    pid: u32,
    children: &HashMap<u32, Vec<u32>>,
    table: &ProcTable,
    my_uid: u32,
    out: &mut Vec<u32>,
) {
    let mut kids = children.get(&pid).cloned().unwrap_or_default();
    kids.sort_unstable();
    for kid in kids {
        if table.get(&kid).is_some_and(|p| p.uid == Some(my_uid)) {
            collect_subtree(kid, children, table, my_uid, out);
        }
    }
    out.push(pid);
}

fn is_ancestor(candidate: u32, mut pid: u32, table: &ProcTable) -> bool {
    // Bounded walk in case of a cycle from a racy snapshot.
    for _ in 0..table.len() {
        match table.get(&pid).and_then(|p| p.ppid) {
            Some(parent) if parent == candidate => return true,
            Some(parent) if parent > 1 => pid = parent,
            _ => return false,
        }
    }
    false
}

fn is_wrapper(p: &ProcInfo) -> bool {
    let program = identity::program(&p.cmd, &p.name);
    if MULTI_RUNNERS.contains(&program.as_str()) {
        return false;
    }
    if SHELLS.contains(&program.as_str()) {
        // `sh -c "vite"` is a wrapper; an interactive shell is the user's terminal.
        return p.cmd.iter().skip(1).any(|a| a == "-c");
    }
    WRAPPERS.contains(&program.as_str()) || WRAPPERS.contains(&p.name.as_str())
}

/// Snapshot of every process (ppid, name, argv, uid). Only taken when the user
/// presses kill, since a full scan is the expensive path on macOS.
pub fn snapshot() -> ProcTable {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cmd(UpdateKind::Always)
            .with_user(UpdateKind::Always),
    );
    sys.processes()
        .iter()
        .map(|(pid, p)| {
            let info = ProcInfo {
                ppid: p.parent().map(Pid::as_u32),
                name: p.name().to_string_lossy().into_owned(),
                cmd: p
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect(),
                uid: p.user_id().map(|u| **u),
            };
            (pid.as_u32(), info)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Waiting,
    /// Everything exited. `forced` processes needed SIGKILL.
    Done {
        killed: usize,
        forced: usize,
    },
    /// Still alive after SIGKILL (e.g. stuck in uninterruptible I/O).
    Stuck {
        survivors: Vec<u32>,
    },
}

/// SIGTERM now, SIGKILL whatever is left after `grace`, give up after `2 × grace`.
pub struct KillJob {
    /// `(pid, start time)`: the start time guards against PID reuse.
    targets: Vec<(u32, u64)>,
    started: Instant,
    grace: Duration,
    forced: usize,
    escalated: bool,
    sys: System,
}

pub const DEFAULT_GRACE: Duration = Duration::from_secs(2);

impl KillJob {
    pub fn start(plan: &Plan, grace: Duration) -> Self {
        let mut sys = System::new();
        let pids: Vec<Pid> = plan.pids.iter().map(|p| Pid::from_u32(*p)).collect();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing().without_tasks(),
        );
        let targets: Vec<(u32, u64)> = plan
            .pids
            .iter()
            .filter_map(|pid| Some((*pid, sys.process(Pid::from_u32(*pid))?.start_time())))
            .collect();
        for (pid, _) in &targets {
            signal(*pid, libc::SIGTERM);
        }
        Self {
            targets,
            started: Instant::now(),
            grace,
            forced: 0,
            escalated: false,
            sys,
        }
    }

    pub fn poll(&mut self) -> Progress {
        let alive = self.alive();
        let total = self.targets.len();
        if alive.is_empty() {
            return Progress::Done {
                killed: total,
                forced: self.forced,
            };
        }
        let elapsed = self.started.elapsed();
        if !self.escalated && elapsed >= self.grace {
            self.escalated = true;
            self.forced = alive.len();
            for pid in &alive {
                signal(*pid, libc::SIGKILL);
            }
        } else if self.escalated && elapsed >= self.grace * 2 {
            return Progress::Stuck { survivors: alive };
        }
        Progress::Waiting
    }

    /// Blocks until the job finishes; for the CLI and tests.
    pub fn wait(mut self) -> Progress {
        loop {
            match self.poll() {
                Progress::Waiting => std::thread::sleep(Duration::from_millis(25)),
                done => return done,
            }
        }
    }

    fn alive(&mut self) -> Vec<u32> {
        let pids: Vec<Pid> = self
            .targets
            .iter()
            .map(|(p, _)| Pid::from_u32(*p))
            .collect();
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing().without_tasks(),
        );
        self.targets
            .iter()
            .filter(|(pid, start)| {
                self.sys.process(Pid::from_u32(*pid)).is_some_and(|p| {
                    p.start_time() == *start && p.status() != ProcessStatus::Zombie
                })
            })
            .map(|(pid, _)| *pid)
            .collect()
    }
}

fn signal(pid: u32, sig: libc::c_int) {
    if let Ok(pid) = libc::pid_t::try_from(pid) {
        // SAFETY: plain syscall; failures (ESRCH = already gone) are fine to ignore
        // because `alive` re-checks the real state.
        unsafe { libc::kill(pid, sig) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: u32 = 501;

    fn proc_(ppid: u32, cmd: &str, uid: u32) -> ProcInfo {
        let cmd: Vec<String> = cmd.split(' ').map(str::to_owned).collect();
        ProcInfo {
            ppid: Some(ppid),
            name: cmd[0].rsplit('/').next().unwrap().to_string(),
            cmd,
            uid: Some(uid),
        }
    }

    /// launchd(1) → Terminal(50) → zsh(100) → npm(200) → sh -c(300) → vite(400) → esbuild(500)
    ///                                  └→ portwho(900)
    fn npm_tree() -> ProcTable {
        HashMap::from([
            (1, proc_(0, "/sbin/launchd", 0)),
            (50, proc_(1, "/Applications/Terminal", ME)),
            (100, proc_(50, "-zsh", ME)),
            (
                200,
                proc_(
                    100,
                    "node /usr/lib/node_modules/npm/bin/npm-cli.js run dev",
                    ME,
                ),
            ),
            (300, proc_(200, "sh -c vite", ME)),
            (400, proc_(300, "node /app/node_modules/.bin/vite", ME)),
            (
                500,
                proc_(400, "/app/node_modules/esbuild/bin/esbuild --service", ME),
            ),
            (900, proc_(100, "portwho", ME)),
        ])
    }

    #[test]
    fn kills_wrapper_chain_and_descendants_but_not_the_shell() {
        let plan = plan(400, &npm_tree(), ME, 900).unwrap();
        assert_eq!(plan.root, 200);
        assert_eq!(plan.pids, [500, 400, 300, 200], "deepest first");
        assert_eq!(plan.chain, ["npm", "sh", "vite"]);
    }

    #[test]
    fn stops_at_multi_process_runners() {
        let mut t = npm_tree();
        t.insert(
            200,
            proc_(
                100,
                "node /app/node_modules/concurrently/dist/bin.js a b",
                ME,
            ),
        );
        t.insert(600, proc_(200, "sh -c api", ME));
        let plan = plan(400, &t, ME, 900).unwrap();
        assert_eq!(
            plan.root, 300,
            "concurrently and its other children survive"
        );
        assert_eq!(plan.pids, [500, 400, 300]);
    }

    #[test]
    fn stops_at_wrappers_that_launched_several_children() {
        let mut t = npm_tree();
        // `sh -c "vite & node api.js"`: two servers under one shell.
        t.insert(301, proc_(300, "node api.js", ME));
        let plan = plan(400, &t, ME, 900).unwrap();
        assert_eq!(plan.root, 400);
        assert_eq!(plan.pids, [500, 400]);
        assert_eq!(plan.chain, ["vite"]);
    }

    #[test]
    fn standalone_process_is_its_own_root() {
        let t = HashMap::from([
            (100, proc_(1, "-zsh", ME)),
            (400, proc_(100, "python3 -m http.server", ME)),
        ]);
        let plan = plan(400, &t, ME, 9999).unwrap();
        assert_eq!((plan.root, plan.pids), (400, vec![400]));
    }

    #[test]
    fn skips_descendants_owned_by_others() {
        let mut t = npm_tree();
        t.insert(700, proc_(400, "/usr/bin/sudo something", 0));
        let plan = plan(400, &t, ME, 900).unwrap();
        assert!(!plan.pids.contains(&700));
    }

    #[test]
    fn refusals() {
        let mut t = npm_tree();
        t.insert(80, proc_(1, "/usr/sbin/sshd", 0));
        t.insert(81, proc_(1, "com.docker.backend", ME));
        assert_eq!(plan(12345, &t, ME, 900), Err(Refusal::Gone));
        assert_eq!(plan(80, &t, ME, 900), Err(Refusal::NotMine));
        assert_eq!(plan(1, &t, 0, 900), Err(Refusal::Protected));
        assert_eq!(
            plan(81, &t, ME, 900),
            Err(Refusal::Container("Docker Desktop"))
        );
        // An editor's node server that is also portwho's terminal host.
        assert_eq!(plan(50, &t, ME, 900), Err(Refusal::OwnAncestor));
        assert_eq!(plan(900, &t, ME, 900), Err(Refusal::OwnAncestor));
    }

    #[test]
    fn interactive_shell_is_not_a_wrapper() {
        assert!(!is_wrapper(&proc_(1, "-zsh", ME)));
        assert!(!is_wrapper(&proc_(1, "/bin/bash", ME)));
        assert!(is_wrapper(&proc_(1, "/bin/sh -c next dev", ME)));
        assert!(is_wrapper(&proc_(1, "nodemon server.js", ME)));
        assert!(!is_wrapper(&proc_(1, "/usr/bin/tmux", ME)));
    }
}
