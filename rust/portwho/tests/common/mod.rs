#![allow(dead_code)] // each test crate uses a different subset

//! A real dev-server-shaped fixture: `sh -c "python3 …; true"` listening on a
//! random localhost port, started inside a project folder with a package.json.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

const SERVER: &str = r"
import signal, socket, sys, time
if 'stubborn' in sys.argv:
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
s = socket.socket()
s.bind(('127.0.0.1', 0))
s.listen()
print(s.getsockname()[1], flush=True)
time.sleep(120)
";

pub struct Fixture {
    pub port: u16,
    /// The `sh -c` wrapper.
    pub shell_pid: u32,
    /// The python process that owns the socket.
    pub server_pid: u32,
    pub dir: PathBuf,
    child: Child,
}

impl Fixture {
    /// `stubborn` servers ignore SIGTERM, forcing the SIGKILL escalation.
    pub fn start(name: &str, stubborn: bool) -> Self {
        let dir = std::env::temp_dir().join(format!("portwho-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            format!(r#"{{"name":"{name}","private":true}}"#),
        )
        .unwrap();

        // `; true` stops sh from exec-ing python, so the wrapper stays in the tree.
        let mut child = Command::new("sh")
            .args(["-c", r#"python3 -c "$PW_SERVER" $PW_MODE; true"#])
            .env("PW_SERVER", SERVER)
            .env("PW_MODE", if stubborn { "stubborn" } else { "polite" })
            .current_dir(&dir)
            .stdout(Stdio::piped())
            .spawn()
            .expect("python3 is required for the e2e tests");

        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port: u16 = line.trim().parse().expect("fixture should print its port");
        let shell_pid = child.id();
        let server_pid = server_pid(shell_pid);
        Self {
            port,
            shell_pid,
            server_pid,
            dir,
            child,
        }
    }
}

/// The fixture's listener, as seen by portwho's own socket scan.
fn server_pid(shell_pid: u32) -> u32 {
    let table = portwho::kill::snapshot();
    table
        .iter()
        .find(|(_, p)| p.ppid == Some(shell_pid))
        .map(|(pid, _)| *pid)
        .expect("sh should have a python child")
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for pid in [self.server_pid, self.shell_pid] {
            // SAFETY: plain syscall; the pids are our own fixture processes.
            unsafe { libc::kill(pid.try_into().unwrap(), libc::SIGKILL) };
        }
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn port_is_listening(port: u16) -> bool {
    portwho::ports::listening_sockets()
        .unwrap()
        .iter()
        .any(|(p, ..)| *p == port)
}
