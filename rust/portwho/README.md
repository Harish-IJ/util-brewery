# portwho

A live, dtop-style "who owns this port?" table for macOS dev machines. Each listening port shows the **project** that owns it, so five `node` processes stop looking identical, and one key frees a port. Intent and design decisions: [`docs/intent/portwho.md`](../../docs/intent/portwho.md).

```
 portwho   4 listening · 3 yours
  PORT▼  BIND           PROJECT                   COMMAND            PID    CPU   MEM     UP
▶ 3000   * v4+v6        acme-api                  node server.js     2618   0.4   61.2M   2d 4h
  5173   localhost v6   acme-web                  vite --port 5173   2660   1.1   88.0M   8m
  5432   * v4+v6        Docker Desktop (container) com.docker.backend 811    0.2   1.1G    3d 1h
```

## Install / run

```sh
cargo install --path .        # puts `portwho` on your PATH
portwho                       # live table (refreshes every 2s)
portwho -i 1                  # refresh every second
portwho --list                # print once and exit
```

## Keys

| key | does |
|---|---|
| `↑↓` `j k` · `PgUp PgDn` · `g G` | move |
| `/` | filter: words (`acme vite`) or `:30` for ports starting with 30. `enter` keeps it, `esc` clears it |
| `s` / `S` | next sort column (port, project, cpu, mem, uptime) / reverse |
| `x` / `del` | free the port: stop the process tree (asks first) |
| `r` | refresh now |
| `?` | help |
| `q` / `ctrl-c` | quit |

## How a row is identified

- **PROJECT:** the `name` from the nearest `package.json` above the process's cwd. The search stops at a git root or `$HOME`. If no name is found it falls back to the shortened cwd (`~/work/api`). Ports forwarded by Docker Desktop, OrbStack, Colima or Podman are labelled with the runtime.
- **COMMAND:** argv with the runtime stripped: `node …/node_modules/.bin/vite --port 5173` → `vite --port 5173`.
- **BIND:** `*` (yellow) is reachable from your network, `localhost` (green) is this Mac only.
- **MEM:** on macOS this is `phys_footprint`, the same number Activity Monitor shows.
- **Greyed rows** belong to root or another user. macOS won't reveal their details without `sudo`.

## What `x` kills

Killing only the listener often isn't enough: `npm run dev` → `sh -c vite` → `node vite` leaves npm behind, and `nodemon` would restart the server. So portwho:

1. Climbs from the listener through **single-child wrappers** (npm, pnpm, yarn, bun, `sh -c`, nodemon, tsx…).
2. Stops at interactive shells, at multi-service runners (concurrently, turbo, pm2…), and at any launcher with several children, so sibling servers survive.
3. Shows the chain (`npm → sh → vite (3 processes)`) and any other ports it will free, then asks `y/n`.
4. Sends SIGTERM to the subtree (deepest first). After 2 s it SIGKILLs anything still alive, checking start times so a recycled PID is never hit.
5. Re-scans and reports `✓ :5173 is free`, or who holds it now if a supervisor restarted it.

It refuses to kill other users' processes, container forwarders (that would stop every container), PID 1, and anything that would take down portwho's own terminal.

## Tests

```sh
cargo test              # unit + TUI + CLI + real-process kill tests (needs python3)
```

- `src/**` unit tests cover the pure logic: command shortening, package.json lookup, bind labels, filter/sort, and kill planning across npm/concurrently/shell trees.
- `tests/e2e.rs` starts a real `sh -c → python3` server in a temp project. It checks discovery and identification, that SIGTERM frees the port along with the wrapper shell, and that SIGKILL escalation works on a server that ignores SIGTERM.
- `tests/tui.rs` drives key handling and renders frames with ratatui's `TestBackend`.
- `tests/cli.rs` checks `--list`, `--help`, `--version` and argument errors.

CI (`.github/workflows/portwho.yml`) runs fmt, clippy `-D warnings` and every test on **macOS** and Linux.

Release build: ~1 MB binary, ~4 MB RAM, effectively zero idle CPU.
