//! "What is this process?" heuristics: project name, short command, tidy paths.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Interpreters whose first script argument says more than the interpreter itself.
const RUNTIMES: &[&str] = &[
    "node", "bun", "deno", "python", "python3", "ruby", "php", "java",
];

/// Remembers the project found for each cwd so package.json is read once per directory.
#[derive(Default)]
pub struct ProjectCache {
    by_cwd: HashMap<PathBuf, Option<String>>,
}

impl ProjectCache {
    pub fn lookup(&mut self, cwd: &Path) -> Option<String> {
        if let Some(hit) = self.by_cwd.get(cwd) {
            return hit.clone();
        }
        let found = find_project(cwd, home().as_deref());
        self.by_cwd.insert(cwd.to_path_buf(), found.clone());
        found
    }
}

/// Walks up from `cwd` to the nearest `package.json` and returns its `name`
/// (or the folder name for a nameless one). Stops at a git root, at `$HOME`,
/// and at `/`, so a stray `~/package.json` never labels everything.
pub fn find_project(cwd: &Path, home: Option<&Path>) -> Option<String> {
    for dir in cwd.ancestors() {
        if Some(dir) == home || dir.parent().is_none() {
            return None;
        }
        if let Ok(text) = std::fs::read_to_string(dir.join("package.json")) {
            let name = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("name")?.as_str().map(str::to_owned))
                .filter(|n| !n.is_empty());
            return name.or_else(|| Some(dir.file_name()?.to_string_lossy().into_owned()));
        }
        if dir.join(".git").exists() {
            return None;
        }
    }
    None
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// `/Users/me/work/api` → `~/work/api`.
pub fn shorten_path(path: &Path) -> String {
    shorten_path_with(path, home().as_deref())
}

fn shorten_path_with(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Turns a full argv into something recognisable:
/// `/opt/homebrew/bin/node /x/node_modules/.bin/vite --port 5173` → `vite --port 5173`.
pub fn short_command(argv: &[String], fallback: &str) -> String {
    let Some((first, rest)) = argv.split_first() else {
        return fallback.to_string();
    };
    let program = basename(first);

    if RUNTIMES.contains(&program.as_str()) {
        let mut i = 0;
        while i < rest.len() {
            let arg = &rest[i];
            if matches!(arg.as_str(), "-e" | "--eval" | "-p" | "--print" | "-c") {
                // Inline code says nothing useful: `node -e '…'` → `node -e`.
                return format!("{program} {arg}");
            }
            if arg == "-m" && i + 1 < rest.len() {
                // python -m uvicorn app:app
                return join(&rest[i + 1], &rest[i + 2..]);
            }
            if !arg.starts_with('-') {
                // A path-like arg is a script; anything else is a subcommand (`bun run dev`).
                if arg.contains('/') || arg.contains('.') {
                    return join(&script_label(arg), &rest[i + 1..]);
                }
                break;
            }
            i += 1;
        }
    }
    join(&program, rest)
}

/// `node_modules/.bin/vite` → `vite`, `node_modules/@scope/pkg/bin/x.js` → `@scope/pkg`,
/// `/usr/lib/node_modules/npm/bin/npm-cli.js` → `npm`, `server.js` → `server.js`.
fn script_label(script: &str) -> String {
    if let Some((_, after)) = script.rsplit_once("node_modules/.bin/") {
        return after.to_string();
    }
    if let Some((_, after)) = script.rsplit_once("node_modules/") {
        let mut parts = after.split('/');
        let first = parts.next().unwrap_or_default();
        return if first.starts_with('@') {
            format!("{first}/{}", parts.next().unwrap_or_default())
        } else {
            first.to_string()
        };
    }
    basename(script)
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn join(head: &str, tail: &[String]) -> String {
    std::iter::once(head)
        .chain(tail.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn short_commands() {
        let cases = [
            (
                "/opt/homebrew/bin/node /Users/me/app/node_modules/.bin/vite --port 5173",
                "vite --port 5173",
            ),
            (
                "node /Users/me/app/node_modules/next/dist/bin/next dev",
                "next dev",
            ),
            (
                "node /Users/me/app/node_modules/@nestjs/cli/bin/nest.js start --watch",
                "@nestjs/cli start --watch",
            ),
            (
                "/usr/local/bin/node /usr/local/lib/node_modules/npm/bin/npm-cli.js run dev",
                "npm run dev",
            ),
            ("node --inspect server.js", "server.js"),
            (
                "/usr/bin/python3 -m uvicorn app:app --reload",
                "uvicorn app:app --reload",
            ),
            ("/usr/sbin/sshd -D", "sshd -D"),
            (
                "node -e require('http').createServer().listen(3000)",
                "node -e",
            ),
            ("bun run dev", "bun run dev"),
            ("python3 manage.py runserver", "manage.py runserver"),
        ];
        for (input, expected) in cases {
            assert_eq!(short_command(&argv(input), "x"), expected, "{input}");
        }
        assert_eq!(short_command(&[], "postgres"), "postgres");
    }

    #[test]
    fn shortens_home() {
        let home = Path::new("/Users/me");
        assert_eq!(
            shorten_path_with(Path::new("/Users/me/work/api"), Some(home)),
            "~/work/api"
        );
        assert_eq!(shorten_path_with(home, Some(home)), "~");
        assert_eq!(
            shorten_path_with(Path::new("/usr/bin/x"), Some(home)),
            "/usr/bin/x"
        );
    }

    #[test]
    fn finds_nearest_package_json() {
        let root = std::env::temp_dir().join(format!("portwho-test-{}", std::process::id()));
        let app = root.join("home/work/mono/apps/web/src");
        std::fs::create_dir_all(&app).unwrap();
        let home = root.join("home");
        std::fs::write(home.join("package.json"), r#"{"name":"stray"}"#).unwrap();
        std::fs::write(home.join("work/mono/package.json"), r#"{"private":true}"#).unwrap();
        std::fs::write(
            home.join("work/mono/apps/web/package.json"),
            r#"{"name":"@acme/web","dependencies":{"name":"nope"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(home.join("work/plain/.git")).unwrap();

        assert_eq!(
            find_project(&app, Some(&home)).as_deref(),
            Some("@acme/web")
        );
        assert_eq!(
            find_project(&home.join("work/mono"), Some(&home)).as_deref(),
            Some("mono")
        );
        assert_eq!(find_project(&home.join("work/plain"), Some(&home)), None);
        assert_eq!(find_project(&home.join("work"), Some(&home)), None);

        std::fs::remove_dir_all(&root).unwrap();
    }
}
