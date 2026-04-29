# Terminal Emulator (Rust)

Production-oriented terminal emulator core with a Unix PTY backend, ANSI parser, grid model, and dirty-cell renderer.

## Overview
- Spawns an interactive shell inside a pseudo-terminal (PTY).
- Parses terminal output (ANSI/VT control sequences) into a screen grid.
- Renders only dirty cells for efficient updates.
- Captures keyboard + resize events and forwards them to the PTY.

## Architecture
- `src/main.rs`: Runtime orchestration; async event loop; lifecycle cleanup.
- `src/pty/`: PTY creation, shell spawning, resize ioctl, read/write and child reaping.
- `src/ansi/`: Parser state machine for CSI/SGR/control codes.
- `src/terminal/`: Screen buffer and renderer.
- `src/input/`: Raw terminal input + resize event pump.
- `src/config.rs`: TOML config loading with defaults.

## PTY subsystem
1. `openpty` allocates master/slave.
2. Child process `fork`s, `setsid`s, wires slave to stdin/stdout/stderr, then `execvp`s shell.
3. Parent keeps master fd via `PtyMaster` RAII wrapper.
4. On resize events, `TIOCSWINSZ` updates kernel PTY size.
5. On shutdown, child is reaped (`waitpid`) to avoid zombies.

## Input + rendering flow
1. Keyboard events are encoded to bytes.
2. Bytes are written to PTY master.
3. Shell output is read from PTY (blocking read in `spawn_blocking`).
4. Parser mutates `Grid` and marks dirty cells.
5. Renderer paints only dirty cells, then dirty flags reset.

## Build and run
Prereqs:
- Rust stable toolchain
- Unix-like OS (Linux/macOS) with PTY support

Commands:
```bash
cargo build
cargo run
cargo test
cargo clippy --all-targets --all-features
```

## Configuration
Config path:
- `$XDG_CONFIG_HOME/terminal_emulator/config.toml`
- or `~/.config/terminal_emulator/config.toml`

Example:
```toml
shell = "/bin/sh"
rows = 24
cols = 80
font_size = 14
color_scheme = "dark"
```

## Interview-ready architecture summary
This project cleanly separates concerns across transport (PTY), protocol (ANSI parser), state model (grid), and presentation (renderer), while using RAII and typed errors to enforce safe resource lifetime.
