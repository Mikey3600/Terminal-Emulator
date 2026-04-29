# Terminal Emulator (Rust)

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

- Rust stable toolchain (`rustup`, `cargo`)
- Unix-like OS with PTY support (Linux/macOS)

### Commands

```bash
cargo build
cargo run
```

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

## License

MIT
