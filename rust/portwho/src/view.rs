//! Filtering and sorting: which entries are visible, in what order.

use std::cmp::Ordering;

use crate::ports::PortEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Port,
    Project,
    Cpu,
    Memory,
    Uptime,
}

impl SortKey {
    pub const ALL: [Self; 5] = [
        Self::Port,
        Self::Project,
        Self::Cpu,
        Self::Memory,
        Self::Uptime,
    ];

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Port => "port",
            Self::Project => "project",
            Self::Cpu => "cpu",
            Self::Memory => "mem",
            Self::Uptime => "uptime",
        }
    }

    /// The direction that's useful first: busiest / biggest / oldest at the top.
    fn natural(self, a: &PortEntry, b: &PortEntry) -> Ordering {
        match self {
            Self::Port => a.port.cmp(&b.port),
            Self::Project => a.label().to_lowercase().cmp(&b.label().to_lowercase()),
            Self::Cpu => b.cpu.total_cmp(&a.cpu),
            Self::Memory => b.mem_bytes.cmp(&a.mem_bytes),
            Self::Uptime => b.uptime_secs.cmp(&a.uptime_secs),
        }
    }
}

/// `:30` matches ports starting with 30. Anything else is case-insensitive words
/// that must all appear in port, project, command, process name or pid.
pub fn matches(e: &PortEntry, query: &str) -> bool {
    let query = query.trim();
    if let Some(port) = query.strip_prefix(':') {
        return e.port.to_string().starts_with(port.trim());
    }
    let haystack = format!(
        "{} {} {} {} {}",
        e.port,
        e.label(),
        e.short_cmd(),
        e.name,
        e.pid
    )
    .to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

/// Indices into `entries` of the rows to show, in display order.
pub fn visible(entries: &[PortEntry], query: &str, key: SortKey, reverse: bool) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..entries.len())
        .filter(|&i| matches(&entries[i], query))
        .collect();
    idx.sort_by(|&a, &b| {
        let (ea, eb) = (&entries[a], &entries[b]);
        let ord = key
            .natural(ea, eb)
            .then(ea.port.cmp(&eb.port))
            .then(ea.pid.cmp(&eb.pid));
        if reverse { ord.reverse() } else { ord }
    });
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        port: u16,
        pid: u32,
        project: &str,
        cmd: &str,
        cpu: f32,
        mem: u64,
        up: u64,
    ) -> PortEntry {
        PortEntry {
            port,
            pid,
            name: "node".into(),
            project: Some(project.into()),
            cmd: cmd.split(' ').map(str::to_owned).collect(),
            cpu,
            mem_bytes: Some(mem),
            uptime_secs: Some(up),
            ..Default::default()
        }
    }

    fn sample() -> Vec<PortEntry> {
        vec![
            entry(
                5173,
                30,
                "acme-web",
                "node /w/node_modules/.bin/vite",
                1.0,
                300,
                10,
            ),
            entry(3000, 20, "acme-api", "node server.js", 9.0, 100, 9000),
            entry(8080, 10, "Billing", "python3 -m http.server", 0.0, 200, 50),
        ]
    }

    fn ports(entries: &[PortEntry], idx: &[usize]) -> Vec<u16> {
        idx.iter().map(|&i| entries[i].port).collect()
    }

    #[test]
    fn sorts_each_key_in_its_useful_direction() {
        let e = sample();
        let order = |k| ports(&e, &visible(&e, "", k, false));
        assert_eq!(order(SortKey::Port), [3000, 5173, 8080]);
        assert_eq!(
            order(SortKey::Project),
            [3000, 5173, 8080],
            "case-insensitive"
        );
        assert_eq!(order(SortKey::Cpu), [3000, 5173, 8080]);
        assert_eq!(order(SortKey::Memory), [5173, 8080, 3000]);
        assert_eq!(order(SortKey::Uptime), [3000, 8080, 5173], "oldest first");
        assert_eq!(
            ports(&e, &visible(&e, "", SortKey::Port, true)),
            [8080, 5173, 3000]
        );
    }

    #[test]
    fn filters() {
        let e = sample();
        let f = |q| ports(&e, &visible(&e, q, SortKey::Port, false));
        assert_eq!(f(""), [3000, 5173, 8080]);
        assert_eq!(f(":3"), [3000]);
        assert_eq!(f(": 80"), [8080]);
        assert_eq!(f(":9"), Vec::<u16>::new());
        assert_eq!(f("acme"), [3000, 5173]);
        assert_eq!(f("ACME vite"), [5173], "all words must match");
        assert_eq!(f("billing"), [8080]);
        assert_eq!(f("http.server"), [8080]);
        assert_eq!(f("20"), [3000], "pid");
    }

    #[test]
    fn sort_key_cycles_through_all() {
        let mut k = SortKey::default();
        let mut seen = vec![k];
        for _ in 0..SortKey::ALL.len() - 1 {
            k = k.next();
            seen.push(k);
        }
        assert_eq!(seen, SortKey::ALL);
        assert_eq!(k.next(), SortKey::Port);
    }
}
