# Terminal Emulator (Rust)

A portfolio-grade terminal emulator core implemented in Rust, focused on clean systems design: PTY process management, ANSI parsing, grid state modeling, and incremental rendering.

## Project overview
This project runs an interactive shell inside a pseudo-terminal (PTY), ingests shell output, parses ANSI/VT escape sequences, updates a screen grid, and renders terminal state to the host console. It is intentionally modular so contributors can extend parser behavior, rendering strategy, or input handling independently.

## Architecture
The runtime is split into five layers:
1. **Orchestration (`main.rs`)** — initializes config + runtime, wires channels, runs the event loop.
2. **Transport (`pty/`)** — creates PTY, forks/execs shell, handles read/write/resize, reaps child.
3. **Protocol (`ansi/`)** — interprets control bytes and escape sequences into screen operations.
4. **State (`terminal/screen_buffer.rs`)** — owns cells, cursor, dirty-region tracking, resize behavior.
5. **Presentation (`terminal/renderer.rs`)** — paints only dirty cells for efficient redraws.

## Module structure
- `src/main.rs` — orchestration layer only.
- `src/config.rs` — user config loading and defaults.
- `src/pty/` — PTY master/slave abstractions and process lifecycle.
- `src/ansi/` — parser state machine.
- `src/terminal/` — grid model + renderer.
- `src/input/` — raw keyboard/resize event pump + key encoding.
- `src/utils/` — shared utilities/error aliases.

## PTY subsystem
- Allocates PTY pair via `openpty`.
- Forks the process and `execvp`s configured shell in the child.
- Child rebinds stdio to PTY slave (`dup2`) and enters its own session (`setsid`).
- Parent owns the PTY master via RAII wrapper and performs all I/O.
- Window resize events are forwarded to kernel using `TIOCSWINSZ`.
- Child process is explicitly reaped to avoid zombies.

## Terminal rendering pipeline
1. Read bytes from PTY master.
2. Feed bytes into ANSI parser.
3. Parser mutates `Grid` state and marks dirty cells.
4. Renderer flushes only dirty cells to stdout.
5. Dirty flags are cleared for next frame.

## Input handling flow
1. Crossterm raw mode captures key + resize events.
2. Key events are encoded into terminal byte sequences.
3. Encoded bytes are sent over channel to main loop.
4. Main loop writes bytes into PTY master.
5. Shell responds through PTY output path (rendering pipeline above).

## Installation
### Prerequisites
- Rust stable toolchain (`rustup`, `cargo`)
- Unix-like OS with PTY support (Linux/macOS)

### Setup
```bash
git clone <your-fork-url>
cd Terminal-Emulator
cargo build
```

## Run the project
```bash
cargo run
```

## Example commands (inside emulator)
```text
echo "hello"
ls -la
pwd
vim README.md
```

## Validation commands
```bash
cargo build
cargo run
cargo test
cargo clippy --all-targets --all-features
```

## Future improvements
- Scrollback history with bounded ring buffer integration.
- Wider ANSI/DEC mode coverage and compliance tests.
- Alternate screen support + mouse reporting.
- Better Unicode width/grapheme handling.
- Renderer abstraction for native GUI backends.
