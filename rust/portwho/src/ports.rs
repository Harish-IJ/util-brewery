//! Collects listening TCP sockets and enriches them with process details.
//!
//! `listeners` maps sockets → PIDs (libproc on macOS). `sysinfo` then fills in
//! cwd / argv / CPU / uptime, but only for the handful of PIDs that own a
//! listener, never for every process on the system.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use listeners::{Protocol, SocketState};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::identity::{self, ProjectCache};
use crate::mem;

/// One row in the table: a process listening on a port (all of its binds merged).
#[derive(Debug, Clone, Default)]
pub struct PortEntry {
    pub port: u16,
    pub pid: u32,
    pub binds: Vec<IpAddr>,
    /// Short process name from the kernel (e.g. `node`, `com.docker.backend`).
    pub name: String,
    pub exe: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub cmd: Vec<String>,
    /// `package.json` name of the project the process runs in, if any.
    pub project: Option<String>,
    /// Set when the listener is a container runtime forwarding a published port.
    pub forwarder: Option<&'static str>,
    pub uptime_secs: Option<u64>,
    /// Percent of one core; can exceed 100 for multi-threaded processes.
    pub cpu: f32,
    /// Activity Monitor's "Memory" (`phys_footprint`) on macOS, RSS elsewhere.
    pub mem_bytes: Option<u64>,
    /// False for root / other-user processes, which we can't inspect or kill.
    pub owned_by_me: bool,
}

impl PortEntry {
    /// What the PROJECT column shows: container runtime, package name, shortened cwd, or exe.
    pub fn label(&self) -> String {
        if let Some(fwd) = self.forwarder {
            return format!("{fwd} (container)");
        }
        if let Some(project) = &self.project {
            return project.clone();
        }
        if let Some(cwd) = self.cwd.as_deref().filter(|p| *p != Path::new("/")) {
            return identity::shorten_path(cwd);
        }
        match &self.exe {
            Some(exe) => identity::shorten_path(exe),
            None => format!("({})", self.name),
        }
    }

    pub fn short_cmd(&self) -> String {
        identity::short_command(&self.cmd, &self.name)
    }

    pub fn bind(&self) -> Bind {
        Bind::of(&self.binds)
    }
}

/// Where a port is reachable from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Bound to a wildcard address: reachable from the network.
    All,
    Localhost,
    Specific,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    pub scope: Scope,
    /// `*`, `localhost`, or the specific IP.
    pub host: String,
    /// `v4`, `v6`, or `v4+v6`.
    pub family: &'static str,
}

impl Bind {
    pub fn of(binds: &[IpAddr]) -> Self {
        let canon: Vec<IpAddr> = binds.iter().map(IpAddr::to_canonical).collect();
        let (scope, host) = if canon.iter().any(IpAddr::is_unspecified) {
            (Scope::All, "*".to_string())
        } else if canon.iter().all(IpAddr::is_loopback) {
            (Scope::Localhost, "localhost".to_string())
        } else {
            let ip = canon.iter().find(|ip| !ip.is_loopback());
            (
                Scope::Specific,
                ip.map_or_else(String::new, ToString::to_string),
            )
        };
        let v4 = canon.iter().any(IpAddr::is_ipv4);
        let v6 = canon.iter().any(IpAddr::is_ipv6);
        let family = match (v4, v6) {
            (true, true) => "v4+v6",
            (true, false) => "v4",
            _ => "v6",
        };
        Self {
            scope,
            host,
            family,
        }
    }

    pub fn label(&self) -> String {
        format!("{} {}", self.host, self.family)
    }
}

/// A raw listening TCP socket: `(port, pid, process name, bound IP)`.
pub type RawSocket = (u16, u32, String, IpAddr);

/// Groups sockets by `(port, pid)` so a dual-stack server is a single row.
pub fn group_sockets(
    sockets: impl IntoIterator<Item = RawSocket>,
) -> BTreeMap<(u16, u32), (String, Vec<IpAddr>)> {
    let mut grouped: BTreeMap<(u16, u32), (String, Vec<IpAddr>)> = BTreeMap::new();
    for (port, pid, name, ip) in sockets {
        let entry = grouped
            .entry((port, pid))
            .or_insert_with(|| (name, Vec::new()));
        if !entry.1.contains(&ip) {
            entry.1.push(ip);
        }
    }
    grouped
}

/// Every listening TCP socket on the machine that we're allowed to see.
pub fn listening_sockets() -> Result<Vec<RawSocket>, String> {
    let all = listeners::get_all().map_err(|e| e.to_string())?;
    Ok(all
        .into_iter()
        .filter(|l| l.protocol == Protocol::TCP && l.state == SocketState::Listen)
        .map(|l| {
            (
                l.socket.port(),
                l.process.pid,
                l.process.name,
                l.socket.ip(),
            )
        })
        .collect())
}

/// Owns the long-lived state needed between refreshes (sysinfo's process table, caches).
pub struct Collector {
    sys: System,
    projects: ProjectCache,
    my_uid: u32,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            projects: ProjectCache::default(),
            my_uid: current_uid(),
        }
    }

    pub fn collect(&mut self) -> Result<Vec<PortEntry>, String> {
        let grouped = group_sockets(listening_sockets()?);

        let mut pids: Vec<Pid> = grouped.keys().map(|(_, pid)| Pid::from_u32(*pid)).collect();
        pids.sort_unstable();
        pids.dedup();
        // sysinfo keeps every PID it was ever asked about. Start over once stale
        // entries dominate so a long session doesn't grow (costs one CPU sample).
        if self.sys.processes().len() > 2 * pids.len() + 32 {
            self.sys = System::new();
        }
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_cpu()
                .with_memory()
                .with_cwd(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_user(UpdateKind::OnlyIfNotSet),
        );
        let entries = grouped
            .into_iter()
            .map(|((port, pid), (name, binds))| {
                let proc_ = self.sys.process(Pid::from_u32(pid));
                let cwd = proc_.and_then(|p| p.cwd()).map(Path::to_path_buf);
                let exe = proc_.and_then(|p| p.exe()).map(Path::to_path_buf);
                let cmd: Vec<String> = proc_
                    .map(|p| {
                        p.cmd()
                            .iter()
                            .map(|s| s.to_string_lossy().into_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                let forwarder = identity::forwarder(&name, exe.as_deref(), &cmd);
                let project = match (&forwarder, cwd.as_deref()) {
                    (None, Some(c)) => self.projects.lookup(c),
                    _ => None,
                };
                PortEntry {
                    port,
                    pid,
                    binds,
                    owned_by_me: proc_
                        .and_then(|p| p.user_id())
                        .is_some_and(|uid| **uid == self.my_uid),
                    cpu: proc_.map_or(0.0, sysinfo::Process::cpu_usage),
                    mem_bytes: mem::footprint(pid)
                        .or_else(|| proc_.map(sysinfo::Process::memory))
                        .filter(|b| *b > 0),
                    uptime_secs: proc_.map(sysinfo::Process::run_time),
                    name,
                    exe,
                    cwd,
                    cmd,
                    project,
                    forwarder,
                }
            })
            .collect();
        Ok(entries)
    }
}

pub fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn bind_labels() {
        let cases = [
            (&["0.0.0.0", "::"][..], Scope::All, "* v4+v6"),
            (&["::1"], Scope::Localhost, "localhost v6"),
            (&["127.0.0.1", "::1"], Scope::Localhost, "localhost v4+v6"),
            (&["::ffff:127.0.0.1"], Scope::Localhost, "localhost v4"),
            (&["::ffff:0.0.0.0"], Scope::All, "* v4"),
            (&["192.168.1.5"], Scope::Specific, "192.168.1.5 v4"),
            (&["127.0.0.1", "10.0.0.2"], Scope::Specific, "10.0.0.2 v4"),
        ];
        for (input, scope, label) in cases {
            let bind = Bind::of(&ips(input));
            assert_eq!(
                (bind.scope, bind.label().as_str()),
                (scope, label),
                "{input:?}"
            );
        }
    }

    #[test]
    fn groups_dual_stack_sockets_into_one_row() {
        let sock = |port, pid, ip: &str| (port, pid, "node".to_string(), ip.parse().unwrap());
        let grouped = group_sockets([
            sock(3000, 10, "0.0.0.0"),
            sock(3000, 10, "::"),
            sock(3000, 10, "::"),
            sock(3000, 11, "127.0.0.1"),
            sock(5173, 10, "::1"),
        ]);
        let keys: Vec<_> = grouped.keys().copied().collect();
        assert_eq!(keys, [(3000, 10), (3000, 11), (5173, 10)]);
        assert_eq!(grouped[&(3000, 10)].1, ips(&["0.0.0.0", "::"]));
    }

    #[test]
    fn label_priority() {
        let mut e = PortEntry {
            name: "node".into(),
            exe: Some("/usr/bin/node".into()),
            ..Default::default()
        };
        assert_eq!(e.label(), "/usr/bin/node");
        e.cwd = Some("/".into());
        assert_eq!(e.label(), "/usr/bin/node", "cwd `/` says nothing");
        e.cwd = Some("/srv/api".into());
        assert_eq!(e.label(), "/srv/api");
        e.project = Some("acme-api".into());
        assert_eq!(e.label(), "acme-api");
        e.forwarder = Some("Docker Desktop");
        assert_eq!(e.label(), "Docker Desktop (container)");
        let bare = PortEntry {
            name: "launchd".into(),
            ..Default::default()
        };
        assert_eq!(bare.label(), "(launchd)");
    }
}
