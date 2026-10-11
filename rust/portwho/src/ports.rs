//! Collects listening TCP sockets and enriches them with process details.
//!
//! `listeners` maps sockets → PIDs (libproc on macOS). `sysinfo` then fills in
//! cwd / argv / uptime, but only for the handful of PIDs that own a listener,
//! never for every process on the system.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use listeners::{Protocol, SocketState};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::identity::{self, ProjectCache};

/// One row in the table: a process listening on a port (all of its binds merged).
#[derive(Debug, Clone)]
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
    pub uptime_secs: Option<u64>,
    /// False for root / other-user processes, which we can't inspect or kill.
    pub owned_by_me: bool,
}

impl PortEntry {
    /// What the PROJECT column shows: package name, else the shortened cwd, else the exe.
    pub fn label(&self) -> String {
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

    pub fn bind_label(&self) -> String {
        bind_label(&self.binds)
    }
}

/// Summarises a set of bind addresses: `*` (all interfaces), `localhost`, or the IP,
/// followed by the IP families in use.
pub fn bind_label(binds: &[IpAddr]) -> String {
    let host = if binds.iter().any(IpAddr::is_unspecified) {
        "*".to_string()
    } else if binds.iter().all(is_loopback) {
        "localhost".to_string()
    } else {
        binds
            .iter()
            .find(|ip| !is_loopback(ip))
            .map_or_else(String::new, ToString::to_string)
    };
    let v4 = binds.iter().any(|ip| ip.to_canonical().is_ipv4());
    let v6 = binds.iter().any(|ip| ip.to_canonical().is_ipv6());
    let family = match (v4, v6) {
        (true, true) => "v4+v6",
        (true, false) => "v4",
        _ => "v6",
    };
    format!("{host} {family}")
}

fn is_loopback(ip: &IpAddr) -> bool {
    ip.to_canonical().is_loopback()
}

/// Owns the long-lived state needed between refreshes (sysinfo's process table, caches).
pub struct Collector {
    sys: System,
    projects: ProjectCache,
    my_uid: u32,
}

impl Collector {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            projects: ProjectCache::default(),
            // SAFETY: getuid has no preconditions and cannot fail.
            my_uid: unsafe { libc::getuid() },
        }
    }

    pub fn collect(&mut self) -> Result<Vec<PortEntry>, String> {
        let all = listeners::get_all().map_err(|e| e.to_string())?;

        // Group sockets by (port, pid) so a dual-stack server is one row.
        let mut grouped: BTreeMap<(u16, u32), (String, Vec<IpAddr>)> = BTreeMap::new();
        for l in all {
            if l.protocol != Protocol::TCP || l.state != SocketState::Listen {
                continue;
            }
            let entry = grouped
                .entry((l.socket.port(), l.process.pid))
                .or_insert_with(|| (l.process.name.clone(), Vec::new()));
            if !entry.1.contains(&l.socket.ip()) {
                entry.1.push(l.socket.ip());
            }
        }

        let pids: Vec<Pid> = {
            let mut p: Vec<u32> = grouped.keys().map(|(_, pid)| *pid).collect();
            p.sort_unstable();
            p.dedup();
            p.into_iter().map(Pid::from_u32).collect()
        };
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing()
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
                let project = cwd.as_deref().and_then(|c| self.projects.lookup(c));
                let owned_by_me = proc_
                    .and_then(|p| p.user_id())
                    .is_some_and(|uid| **uid == self.my_uid);
                PortEntry {
                    port,
                    pid,
                    binds,
                    name,
                    exe: proc_.and_then(|p| p.exe()).map(Path::to_path_buf),
                    cwd,
                    cmd: proc_
                        .map(|p| {
                            p.cmd()
                                .iter()
                                .map(|s| s.to_string_lossy().into_owned())
                                .collect()
                        })
                        .unwrap_or_default(),
                    project,
                    uptime_secs: proc_.map(sysinfo::Process::run_time),
                    owned_by_me,
                }
            })
            .collect();
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn bind_labels() {
        assert_eq!(bind_label(&ips(&["0.0.0.0", "::"])), "* v4+v6");
        assert_eq!(bind_label(&ips(&["::1"])), "localhost v6");
        assert_eq!(bind_label(&ips(&["127.0.0.1", "::1"])), "localhost v4+v6");
        assert_eq!(bind_label(&ips(&["::ffff:127.0.0.1"])), "localhost v4");
        assert_eq!(bind_label(&ips(&["192.168.1.5"])), "192.168.1.5 v4");
    }
}
