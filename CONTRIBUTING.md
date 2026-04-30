# Contributing

## Development setup
1. Install stable Rust via `rustup`.
2. Clone and enter the repository.
3. Run `cargo build` and `cargo test`.

## Quality checks before PR
- `cargo fmt -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- `cargo build`

## PR process
- Keep changes focused and documented.
- Add/update tests for behavior changes.
- Include a concise summary and validation steps.
