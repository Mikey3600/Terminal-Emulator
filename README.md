# terminal-emulator

A terminal emulator core written in Rust. Runs a real shell over a PTY, decodes ANSI/VT escape sequences through a finite state machine, maintains a styled screen grid, and renders only what changed.

Not a wrapper around someone else's terminal library. The PTY handling, the parser, the grid model — built from scratch.

---

## Why

I wanted to understand what actually happens between you pressing a key and text appearing on screen. Turns out there's a lot: fork/exec, file descriptor plumbing, a state machine parsing 40-year-old escape codes, dirty cell tracking, Unicode width calculations. Building it in Rust meant the compiler forced every ownership decision to be explicit — which made the hard parts harder to ignore and easier to reason about once solved.

---

## What it does

- Spawns a shell (`bash`, `zsh`, or whatever `$SHELL` points to) inside a PTY
- Reads PTY output and feeds it through an ANSI/VT parser (CSI, SGR, OSC, cursor movement, erase modes)
- Maintains a 2D grid of styled cells with dirty tracking
- Renders incrementally — only changed cells get redrawn
- Handles keyboard input encoding (printable chars, control sequences, arrow keys, Ctrl+C/D/Z)
- Responds to terminal resize via `TIOCSWINSZ` + SIGWINCH

---

## Architecture

```
keyboard / resize
      │
      ▼
 event loop (tokio mpsc)
      │
      ├──► PTY write (key bytes → shell stdin)
      │
      └──► PTY read → ANSI parser → Grid mutation → incremental render
```

The event loop is a single `tokio::mpsc` channel. Three producers (PTY reader, input handler, tick timer) send `TerminalEvent` variants to one consumer. The grid is never touched concurrently — all mutations happen in the event handler, no locks needed.

---

## Project structure

```
src/
├── main.rs              # event loop, task orchestration
├── config.rs            # TOML config + XDG path resolution
├── buffer.rs            # generic ring buffer (used for scrollback)
├── ansi/
│   └── parser.rs        # FSM: Ground → Escape → CSI/OSC → dispatch
├── input/
│   └── keyboard.rs      # raw mode, key encoding, resize events
├── pty/
│   └── pty_master.rs    # openpty, fork/exec, read/write, TIOCSWINSZ, reap
├── terminal/
│   ├── screen_buffer.rs # Cell, Grid, Scrollback, dirty tracking
│   └── renderer.rs      # incremental crossterm render
└── utils/
    └── error.rs         # AppResult type alias
```

---

## Some implementation details worth knowing

**PTY layer** uses `nix` for `openpty` and `fork`, then raw `libc::read`/`libc::write` for I/O. After `fork()`, the child calls `setsid()`, wires `slave_fd` to stdin/stdout/stderr via `dup2`, closes everything else, and `execvp`s the shell. The parent closes `slave_fd` immediately — if it doesn't, the master never sees EOF when the shell exits.

**Parser** is a five-state FSM: `Ground`, `Escape`, `Csi`, `Osc`, `OscEscape`. State is preserved across `feed()` calls so sequences split across PTY reads are handled correctly. Parameters accumulate digit-by-digit with `saturating_mul` to avoid overflow on malformed input.

**Grid** stores cells in a flat `Vec<Cell>` (not `Vec<Vec<Cell>>`). Index arithmetic is `row * cols + col`. Scroll is a single `copy_within` call — one `memmove`, not a loop. Works because `Cell: Copy`.

**Renderer** batches all crossterm commands with `queue!` and flushes once per frame. Skips `MoveTo` when cells are contiguous. Only emits color/attribute changes when they differ from the previous cell.

---

## Known gaps

- 256-color and RGB SGR (`38;5;N`, `38;2;R;G;B`) not yet implemented
- OSC sequences are consumed but not acted on (no window title support)
- Unix only — no Windows ConPTY backend
- No mouse input

---

## Build and run

```bash
# requires stable Rust and a Unix-like OS with PTY support

cargo build
cargo run

cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

---

## Platform

Linux and macOS. The PTY APIs used (`openpty`, `fork`, `ioctl TIOCSWINSZ`) are POSIX — anything Unix-like should work. Windows is not supported.

---


## 11) Conclusion

The project has a solid systems-core design and is well-positioned for iterative hardening. With stronger compatibility testing, Unicode correctness guarantees, and CI/release/security discipline, this can evolve from a strong prototype into a production-capable terminal engine.

---

## 12) Developer Setup (Quick Start)

1. Clone the repo and install stable Rust (`rustup toolchain install stable`).
2. Run local quality checks before opening a PR:
   - `cargo fmt -- --check`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo test`
   - `cargo build`
3. See `CONTRIBUTING.md` for contributor workflow and `SECURITY.md` for vulnerability reporting.

Editor defaults are provided via `.editorconfig`, and Rust formatting defaults are pinned in `rustfmt.toml`.

---

## 13) Architecture Diagram

```text
+------------------+      +--------------------+      +-------------------+
| Input subsystem  | ---> | Event loop (tokio) | ---> | PTY master/slave  |
| keyboard/resize  |      | TerminalEvent mux  |      | shell process I/O |
+------------------+      +---------+----------+      +---------+---------+
                                      |                           |
                                      v                           v
                            +---------+----------+      +---------+---------+
                            | ANSI/VT parser     | ---> | Screen Grid model |
                            | state machine      |      | dirty cell track  |
                            +---------+----------+      +---------+---------+
                                      |                           |
                                      +------------+--------------+
                                                   v
                                         +---------+---------+
                                         | Incremental render |
                                         | crossterm output   |
                                         +--------------------+
```

## 14) Data Flow Notes

- PTY bytes are consumed in frames and fed into parser state transitions.
- Parser dispatch mutates `Grid` cursor/cell state.
- Renderer consumes only dirty cells for minimal redraw.
- Input bytes are encoded and forwarded to the PTY write path.

---

## 15) Extensibility Notes

Current architecture intentionally leaves room for:
- mouse input events (new `TerminalEvent` variants and parser dispatch wiring)
- richer OSC sequence handling (clipboard/title/control channels)
- clipboard integration (event + OSC bridge)
- backend abstraction for Windows ConPTY support

These are additive enhancements aligned with the existing event-driven subsystem boundaries.


## 16) Repository Health Checklist

- Documentation: `README.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, and `SECURITY.md` are present.
- Cargo setup: `Cargo.toml` and `Cargo.lock` are present for reproducible Rust builds.
- Tests & benches: parser fixtures, PTY replay tests, and parser renderer benchmarks are present under `tests/` and `benches/`.
- Project scope: currently focused on a Unix-like PTY-driven terminal core rather than a full GUI terminal emulator.
=======
## Dependencies

| Crate | Purpose |
|---|---|
| `tokio` | async runtime and task scheduling |
| `crossterm` | terminal drawing and raw mode |
| `nix` | PTY creation, fork, Unix process management |
| `libc` | raw syscall bindings (read, write, ioctl) |
| `unicode-segmentation` | grapheme cluster splitting |
| `unicode-width` | display width for wide/CJK characters |
| `serde` + `toml` | config deserialization |
| `thiserror` | custom error types |
| `criterion` | benchmarking |
| `env_logger` | `RUST_LOG`-driven logging |

