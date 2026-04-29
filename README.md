# Terminal Emulator (Rust)

<<<<<<< HEAD
A terminal emulator core written in Rust that runs a real shell through a PTY, parses ANSI/VT escape sequences, maintains a screen grid, and renders updates efficiently using dirty-cell redraws.

---

## Overview

This project focuses on the core runtime of a terminal emulator:

1. Create and manage a pseudo-terminal (PTY)
2. Spawn and control an interactive shell process
3. Read shell output bytes continuously
4. Parse control/escape sequences into terminal actions
5. Update in-memory screen state
6. Render only changed cells to the host terminal
7. Capture keyboard input in raw mode and forward to shell

It is modular and intended to be easy to extend (parser behavior, renderer strategy, input mapping, and config).

---

## Current Features

- **PTY-backed shell session**
  - Uses Unix PTY primitives to spawn a shell in a child process
  - Parent process owns PTY master and performs runtime I/O
  - Handles terminal resize via `TIOCSWINSZ`
  - Reaps child process to avoid zombies

- **ANSI/VT parser**
  - Stateful parser for control bytes and escape sequences
  - Supports cursor motion and positioning CSI commands
  - Supports erase display/line operations
  - Supports SGR styling including basic colors, 256-color, and RGB forms
  - Handles OSC string termination by BEL and `ESC \\`

- **Screen buffer model**
  - 2D grid of cells with style attributes
  - Cursor state and bounds-safe movement
  - Dirty-cell tracking for incremental rendering
  - Resize behavior preserving overlapping content

- **Renderer**
  - Renders only modified cells for efficient redraw
  - Clears dirty markers after each flush

- **Input system**
  - Raw-mode keyboard event capture
  - Encodes common keys into terminal byte sequences
  - Sends encoded input through channels to PTY write path

- **Configuration**
  - TOML-based config loading with defaults
  - Configurable shell path and terminal dimensions

- **Test coverage (current)**
  - Parser tests
  - Screen buffer behavior tests
  - PTY spawn/echo and resize tests

---

## Tech Stack

- **Language:** Rust (Edition 2021)
- **Runtime:** Tokio
- **Terminal I/O:** Crossterm
- **PTY + process control:** nix + libc
- **Config:** serde + toml
- **Errors:** thiserror
- **Logging:** log + env_logger

---

## Project Structure

```text
src/
├── main.rs                 # App orchestration and event loop
├── config.rs               # Config structs + loading/default logic
├── buffer.rs               # Ring buffer utility
├── ansi/
│   ├── mod.rs
│   └── parser.rs           # ANSI/VT parser state machine
├── input/
│   ├── mod.rs
│   └── keyboard.rs         # Raw keyboard capture + encoding
├── pty/
│   ├── mod.rs
│   ├── pty_master.rs       # PTY master lifecycle + read/write/resize
│   └── pty_slave.rs        # PTY slave setup details
├── terminal/
│   ├── mod.rs
│   ├── screen_buffer.rs    # Grid/cell model and cursor logic
│   └── renderer.rs         # Incremental terminal rendering
└── utils/
    ├── mod.rs
    └── error.rs            # Shared error types/aliases
```

---

## How It Works (Data Flow)

1. `main.rs` initializes config and runtime.
2. PTY layer starts shell and exposes master endpoint.
3. Output bytes from PTY are streamed to ANSI parser.
4. Parser mutates `Grid` state in screen buffer.
5. Renderer paints only dirty cells to stdout.
6. Keyboard events are encoded and written back to PTY.

---

## Prerequisites
=======
A modular terminal emulator core written in Rust, with PTY process management, ANSI/VT parsing, grid state modeling, and incremental rendering.

## Current maturity snapshot (as of 2026-04-29)

This project is a **strong systems-programming foundation** and a credible portfolio-grade codebase. It is **not yet production-ready** for broad end-user distribution, but it can reach a 2028-ready baseline with targeted hardening in testing, compatibility, security, release engineering, and observability.

## What exists today

- PTY lifecycle management (`openpty`, `fork`, `setsid`, `dup2`, `execvp`) with explicit child reaping.
- ANSI parser with common control-flow handling and CSI/OSC support.
- Screen grid model with dirty-cell tracking and efficient redraw behavior.
- Raw keyboard input pipeline and PTY write path.
- Config loading via TOML with defaults.
- Unit/integration-style tests for parser, grid semantics, and PTY behavior.

## 2028 industry-standard readiness assessment

### Overall verdict

- **Architecture quality:** Good
- **Maintainability:** Good
- **Core correctness confidence:** Moderate
- **Production operations readiness:** Early
- **Security/compliance posture:** Early

### Must-have gaps to close before claiming “2028-ready”

1. **Standards/compliance coverage**
   - Expand VT/DEC compatibility and add formal conformance fixtures (escape sequence corpus + golden snapshots).
2. **Unicode correctness**
   - Proper UTF-8 decoding, grapheme clusters, East Asian width, combining marks, emoji ZWJ handling.
3. **Cross-platform strategy**
   - Either implement Windows backend (ConPTY) or explicitly scope platform support and release policy.
4. **Security hardening**
   - Add threat model, dependency-vulnerability scanning, SBOM generation, and release artifact signing.
5. **Observability**
   - Structured logs, runtime metrics, and reproducible bug-report bundles.
6. **Release engineering**
   - CI matrix (OS/toolchain), reproducible builds, semantic versioning policy, changelog discipline.
7. **Performance and stress validation**
   - Benchmarks (parser throughput, render latency) and long-run soak tests.

### Recommended roadmap

#### Phase 1 (0–3 months): baseline hardening
- Enforce `cargo fmt`, `clippy -D warnings`, and `cargo test` in CI.
- Add platform matrix (Linux/macOS) and minimum supported Rust version policy.
- Add README security policy section and contribution/testing standards.

#### Phase 2 (3–6 months): correctness + compatibility
- Add ANSI/VT conformance suite with fixture playback.
- Add Unicode-aware rendering pipeline and width calculation tests.
- Add resize (`SIGWINCH`) and alternate screen behavior parity tests.

#### Phase 3 (6–12 months): production readiness
- Add supply-chain controls (cargo-audit, cargo-deny, SBOM).
- Add performance benchmarks + regressions gates.
- Add stable release process (tags, signed artifacts, release notes template).

## Repository structure

- `src/main.rs` — orchestration layer/event loop.
- `src/pty/` — PTY abstractions and process lifecycle.
- `src/ansi/` — parser state machine.
- `src/terminal/` — grid model and renderer.
- `src/input/` — keyboard and resize event capture.
- `src/config.rs` — config loading/defaults.
- `src/buffer.rs` — ring buffer utility.

## Build and run

### Prerequisites
>>>>>>> origin/main

- Rust stable toolchain (`rustup`, `cargo`)
- Unix-like OS with PTY support (Linux/macOS)

<<<<<<< HEAD
---

## Build, Run, and Validate

```bash
# Build
cargo build

# Run
=======
### Commands

```bash
cargo build
>>>>>>> origin/main
cargo run

<<<<<<< HEAD
# Format check
cargo fmt -- --check

# Lints
cargo clippy --all-targets --all-features

# Tests
cargo test
```

---

## Configuration

The project supports TOML configuration (with defaults). Primary fields currently used:

- `shell` — shell binary/path
- `rows` — terminal row count
- `cols` — terminal column count

Additional config fields may exist for planned UI/theming expansion.

---

## Current Limitations

- Terminal compatibility is partial compared with mature emulators
- Advanced Unicode/grapheme-width behavior is not fully complete
- Feature set is core-focused (no full GUI frontend)
- Some modules/functions are present but not fully wired into all runtime paths yet

---

## Future Improvements

- Full UTF-8 + grapheme-cluster aware rendering
- Wider ANSI/DEC compatibility and conformance test corpus
- Alternate screen buffer support (`smcup` / `rmcup`)
- Mouse reporting and richer input protocols
- Scrollback navigation UX improvements
- Stronger CI gates (`clippy -D warnings`, coverage, benchmark checks)
- Dependency/security automation (`cargo-audit`, `cargo-deny`)
- Optional GUI renderer backend
- Cross-platform strategy expansion (including Windows backend path)

---
=======
## Validation commands

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features
cargo test
```

## Known limitations

- Incomplete terminal compatibility relative to mature emulators.
- Limited Unicode/rendering completeness for advanced scripts.
- No hardened release pipeline yet (signing/SBOM/automated security gates).

## README completeness verdict

For a portfolio/research project: **sufficient**.

For a production-grade 2028 claim: **not sufficient unless you maintain these sections continuously**:
- Security policy and vulnerability disclosure channel.
- Supported platforms/version policy (including deprecation policy).
- CI quality gates and release process.
- Compatibility scope (what ANSI/VT behavior is intentionally unsupported).
- Performance targets and benchmark methodology.
>>>>>>> origin/main

## License

MIT
