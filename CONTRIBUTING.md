# Contributing to PRX Voice Client

Thank you for your interest in contributing!

## Prerequisites

- Rust stable 1.85+ (edition 2024)
- macOS / Linux / Windows
- For iOS: Xcode 15+, `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`
- For Android: NDK r25+, `cargo install cargo-ndk`

## Building

```bash
cargo build --workspace          # Build all crates
cargo test --workspace           # Run all tests
cargo run -p prxvc-desktop       # Run desktop app
cargo clippy --workspace -- -D warnings  # Lint
cargo fmt --check                # Format check
```

## Code Standards

This project follows strict production-grade Rust standards:

1. **NO** `unwrap()`, `expect()`, `todo!()`, `unimplemented!()` in production code
2. **NO** dead code (unused variables, imports, parameters)
3. Explicit error handling with `thiserror` and `?` operator
4. Use `parking_lot::Mutex`, not `std::sync::Mutex`
5. Audio thread and UI thread strictly isolated
6. All state changes through the session state machine
7. Doc comments on all public items

## Pull Request Process

1. Fork the repo and create a feature branch
2. Make your changes following the code standards above
3. Ensure all tests pass: `cargo test --workspace`
4. Ensure no clippy warnings: `cargo clippy --workspace -- -D warnings`
5. Ensure formatting: `cargo fmt`
6. Open a PR with a clear description answering:
   - What problem does this solve?
   - Does it change the state machine or protocol?
   - Does it change the FFI contract?
   - How was it tested?

## Commit Messages

- Use English for commit messages
- Use conventional format: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `ci:`
- Keep the first line under 72 characters

## Dependency Upgrade Policy

- Direct dependencies are reviewed monthly
- Security patches applied immediately
- Platform-sensitive deps (cpal, egui, tungstenite) undergo gray-scale upgrade
- Audio regression suite must pass before any audio-related dep upgrade

## Architecture

See [docs/architecture.md](docs/architecture.md) for the crate map and design overview.

## License

By contributing, you agree that your contributions will be licensed under the MIT OR Apache-2.0 dual license.
