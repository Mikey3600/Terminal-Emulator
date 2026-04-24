# Terminal_emulator
 
A terminal emulator written from scratch in Rust. Implements PTY creation, ANSI escape code parsing, a screen grid renderer, keyboard input handling, a scrollback ring buffer, and TOML-based configuration — all wired together with an async Tokio event loop.
 
---
 
## Features
 
- **PTY-backed shell** — spawns a real shell (`/bin/bash`) connected via a Unix pseudo-terminal
- **ANSI / VT100 parser** — finite state machine that handles cursor movement, SGR colors (8-color, 256-color, true color), erase commands, and OSC sequences
- **Dirty-cell rendering** — only redraws cells that changed since the last frame (efficient, flicker-free)
- **Async I/O** — PTY reads and keyboard input run concurrently via `tokio::select!`
- **Raw mode keyboard input** — every keypress is captured and encoded into the correct byte sequence for the shell
- **Scrollback buffer** — fixed-capacity ring buffer stores the last 1000 scrolled-off rows in O(1)
- **TOML configuration** — shell path, terminal dimensions, font size, and color scheme are all user-configurable
---
 
## Project Structure
 
```
terminal_emulator/
├── Cargo.toml
├── .gitignore
├── README.md
└── src/
    ├── main.rs       # Entry point — wires all modules, runs the event loop
    ├── pty.rs        # PTY creation, shell spawning, raw fd read/write
    ├── grid.rs       # Screen grid (2D array of cells), cursor, scroll
    ├── parser.rs     # ANSI escape code parser (finite state machine)
    ├── input.rs      # Raw mode keyboard capture and key encoding
    ├── buffer.rs     # Generic fixed-capacity ring buffer
    └── config.rs     # TOML config loading with sane defaults
```
 
---
 
## Module Breakdown
 
### `main.rs`
Initialises the logger, loads config, spawns the shell, and runs the main `tokio::select!` loop. PTY reads happen inside `spawn_blocking` (blocking I/O off the async reactor). Dirty cells are rendered to the real terminal via crossterm after each read. On exit, raw mode is restored.
 
### `pty.rs`
Opens a PTY pair with `nix::pty::openpty`. Forks a child process that calls `setsid`, wires its stdio to the PTY slave, and `execvp`s the shell. The parent keeps the master fd wrapped in `PtyMaster` (RAII — closes on drop). Read and write use `libc::read` / `libc::write` directly to avoid nix 0.29's `AsFd` requirement on raw fds. `PtyMaster` is `Send + Sync` so it can be shared via `Arc` across the Tokio thread pool.
 
### `grid.rs`
A flat `Vec<Cell>` indexed as `row * cols + col`. Each `Cell` holds a `char`, an `Attributes` struct (bold, italic, underline, blink, reverse, fg color, bg color), and a `dirty` flag. Key operations:
- `write_char` — writes at the cursor, advances it, wraps lines, scrolls when needed
- `scroll_up` — shifts all rows up by one, clears the bottom row
- `clear` / `clear_line_from_cursor` — erase operations; cleared cells carry the current background color
- `dirty_cells` — iterator over only changed cells for efficient rendering
- `clear_dirty` — resets all dirty flags after a render pass
### `parser.rs`
A byte-at-a-time finite state machine with states: `Ground`, `Escape`, `CsiEntry`, `CsiParam`, `OscString`, `OscEscape`. Handles:
- Printable ASCII written to grid
- Control codes: BEL, BS, TAB, LF, CR
- CSI sequences: cursor movement (A/B/C/D), cursor position (H/f), erase display (J), erase line (K)
- SGR (`m`): bold, italic, underline, blink, reverse, 8 basic colors (30–37, 40–47), 256-color (`38;5;n`), true color (`38;2;r;g;b`)
- OSC strings terminated by BEL (`0x07`) or ST (`ESC \`)
### `input.rs`
Enables raw mode via crossterm. Spawns a Tokio task that polls for key events inside `spawn_blocking` (50 ms timeout) and sends encoded byte sequences over an unbounded mpsc channel. Key encoding covers: printable chars, Ctrl+letter, Enter, Tab, Backspace, Escape, arrow keys, Home, End, PageUp, PageDown, Delete. `restore_terminal` disables raw mode on shutdown.
 
### `buffer.rs`
A generic `RingBuffer<T>` with fixed capacity. Push is O(1) — overwrites the oldest entry when full. Random access by age index is O(1). Provides an `iter()` from oldest to newest. Used to retain the last 1000 scrolled-off rows as scrollback history.
 
### `config.rs`
Deserialises a TOML file from `$XDG_CONFIG_HOME/terminal_emulator/config.toml` (falls back to `~/.config/terminal_emulator/config.toml`). All fields have defaults so a missing file is fine. Configurable fields:
 
| Field | Default | Description |
|---|---|---|
| `shell` | `$SHELL` or `/bin/bash` | Shell binary to launch |
| `rows` | `24` | Terminal height in rows |
| `cols` | `80` | Terminal width in columns |
| `font_size` | `14` | Font size (reserved for future GUI) |
| `color_scheme` | `"dark"` | Color scheme name (reserved for future use) |
 
---
 
## Dependencies
 
| Crate | Version | Purpose |
|---|---|---|
| `tokio` | 1 (full) | Async runtime, mpsc channels, spawn_blocking |
| `crossterm` | 0.27 | Terminal raw mode, queued rendering, key events |
| `nix` | 0.29 | PTY, fork, setsid, dup2, execvp |
| `libc` | 0.2 | Raw fd read/write, close |
| `serde` | 1 | Deserialise config structs |
| `toml` | 0.8 | Parse TOML config file |
| `thiserror` | 1 | Ergonomic error enums |
| `log` | 0.4 | Structured logging macros |
| `env_logger` | 0.11 | Log output to stderr, controlled by `RUST_LOG` |
 
---
 
## Building and Running
 
**Prerequisites:** Rust toolchain (1.70+), Linux or macOS (PTY requires Unix).
 
```bash
# Clone
git clone https://github.com/your-username/terminal_emulator
cd terminal_emulator
 
# Build
cargo build --release
 
# Run
cargo run --release
 
# With debug logging
RUST_LOG=info cargo run
```
 
---
 
## Configuration
 
Create the config file if you want to override defaults:
 
```bash
mkdir -p ~/.config/terminal_emulator
nano ~/.config/terminal_emulator/config.toml
```
 
```toml
shell = "/bin/zsh"
rows = 40
cols = 120
font_size = 16
color_scheme = "dark"
```
 
---
 
## Platform
 
Linux only. Requires Unix PTY support (`nix` crate). Does not run on Windows.
 
---
 
## Known Limitations
 
- UTF-8 multi-byte characters are not decoded — only ASCII (0x20–0x7E) is rendered
- No mouse support
- No alternate screen buffer (`smcup`/`rmcup`)
- No window resize handling (`SIGWINCH`)
- Scrollback is stored but not yet navigable via keyboard
- `font_size` and `color_scheme` config fields are reserved for a future GUI renderer
---
 
## License
 
MIT
