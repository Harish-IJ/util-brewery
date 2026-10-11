# portwho: intent

> A dtop-style "who owns this port?" TUI for macOS.
> Confirmed through an interview on 2026-10-11. This is the *why*; the code lives in [`rust/portwho/`](../../rust/portwho).

| | |
|---|---|
| **Outcome** | A live, full-screen terminal app that lists every listening port on my Mac, tells me what each one *is*, and frees a port in one keypress. |
| **User** | Me, on my Mac, mostly cleaning up orphaned dev servers (node / vite / next …) from my own projects. |
| **Why** | Tired of typing `lsof` / `kill` incantations and of not knowing which `node` is which. |
| **Row identity** | Project name (from `package.json`), or the shortened location path when there is none. Plus short command, port + bind, uptime, CPU, memory (Activity Monitor's number). |
| **Interaction** | `↑↓` / `j k` move, sort, `/` filter, `x` kill (confirm → SIGTERM the whole process tree → auto SIGKILL after ~2 s → verify the port is free). |
| **Success** | Open it, recognise the stale server, press one key, and the port is ready for the next command. |
| **Constraint** | Lightweight and efficient: binary of a few MB, a few MB of RAM, almost no CPU when idle. |
| **Out of scope (v1)** | Windows/Linux support, Docker container names (the forwarder is labelled honestly instead), sudo relaunch (root/other-user rows are greyed out and can't be killed), UDP, config files/themes, release packaging. |

## Stack decision

**Rust + ratatui 0.30 + crossterm 0.29, `listeners` + `sysinfo`.** Go + Bubble Tea was considered and would be easier to write. It was rejected because the deciding constraint is *lightweight and efficient*:

- No GC, so the binary is roughly 2–5 MB and the app uses a few MB of RAM.
- `listeners` maps sockets to PIDs on macOS natively through libproc. gopsutil historically shells out to `lsof`, so with Go we'd have to write that shim ourselves.
- `sysinfo` refreshes **only** the PIDs that own listening sockets, never every process, which avoids its per-process `KERN_PROCARGS2` cost on macOS.
- dtop, the reference UX, is built on this same stack.

## Milestones

1. ✅ **M1:** table of listening TCP ports with project or path, short command, bind, PID and uptime.
2. ✅ **M2:** live refresh (`-i`), CPU and memory (`phys_footprint` on macOS), sort (`s`/`S`) and filter (`/`, `:port`).
3. ✅ **M3:** `x` stops the process tree (single-child wrappers, so siblings survive), SIGTERM → SIGKILL after 2 s, then verifies the port is free. Other-user, container-forwarder and own-terminal processes are refused.
4. ✅ **M4:** colour-coded bind (`*` vs `localhost`), Docker/OrbStack/Colima/Podman labels, help overlay, idle budget (~1 MB binary, ~4 MB RSS, ~0 idle CPU).

Tests: unit, TUI (`TestBackend`), CLI, and real-process kill tests. CI runs them on macOS and Linux.
