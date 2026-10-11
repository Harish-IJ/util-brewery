# portwho

A live, dtop-style "who owns this port?" table for macOS dev machines. It shows each listening port with the **project** that owns it, so five `node` processes stop looking identical. Intent and roadmap: [`docs/intent/portwho.md`](../../docs/intent/portwho.md).

```
  PORT   BIND             PROJECT        COMMAND             PID     UP
  3000   * v4             acme-api       node server.js      2618    2d 4h
▶ 5173   localhost v4     acme-web       vite --port 5173    2660    8s
```

## Run

```sh
cargo run --release          # live table
cargo run --release -- -l    # print once and exit
cargo install --path .       # put `portwho` on your PATH
```

Keys: `↑↓`/`j k` move · `g`/`G` top/bottom · `r` refresh · `q` quit.

## How a row is identified

- **PROJECT:** the `name` from the nearest `package.json` above the process's cwd. The search stops at a git root or `$HOME`. If no name is found, the shortened cwd is shown instead (`~/work/api`).
- **COMMAND:** the argv with the runtime stripped: `node …/node_modules/.bin/vite --port 5173` → `vite --port 5173`.
- **BIND:** `*` means all interfaces and `localhost` means loopback only, followed by the IP families in use.
- Rows owned by **another user or root** are greyed out. macOS doesn't let a normal user read their cwd or argv.

## Status

- [x] M1: table of listening TCP ports with project, command, bind, PID and uptime
- [ ] M2: live refresh, CPU/memory, sort and filter
- [ ] M3: kill the process tree and verify the port is free
- [ ] M4: polish
