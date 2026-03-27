# CLAUDE.md

## Project Overview

PRX Voice Client — an open-source, cross-platform, real-time voice AI client system.

Core concept: Session-centric AI Native Voice Client. Users wake AI and engage in natural, multi-turn, interruptible real-time voice conversations.

## Architecture

- **Desktop**: Rust + egui/eframe (P0, primary platform)
- **iOS**: Swift/SwiftUI + Rust Core via UniFFI (P1)
- **Android**: Kotlin/Compose + Rust Core via JNI (P1)
- **Shared Core**: Rust 2024 workspace

## Build & Test

```bash
cargo build --workspace
cargo check --workspace
cargo test --workspace
cargo run -p prxvc-desktop        # Run desktop app
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## Crate Map

| Crate | Package Name | Purpose |
|-------|-------------|---------|
| crates/core | prxvc-core | App context, lifecycle, unified errors |
| crates/session | prxvc-session | Session state machine, turns |
| crates/audio | prxvc-audio | Audio frames, VAD, device profiles |
| crates/protocol | prxvc-protocol | Command/event contracts |
| crates/config | prxvc-config | Configuration model, TOML persistence |
| crates/storage | prxvc-storage | Local SQLite storage |
| crates/observe | prxvc-observe | Tracing, metrics, diagnostics |
| crates/security | prxvc-security | Credential store, data sanitization |
| crates/ffi | prxvc-ffi | FFI boundary for iOS/Android |
| crates/desktop-ui | prxvc-desktop-ui | egui view layer |
| crates/testkit | prxvc-testkit | Test utilities, mocks |
| apps/desktop | prxvc-desktop | Desktop binary (egui) |

## Iron Rules

1. **NO** `unwrap()`, `expect()`, `todo!()`, `unimplemented!()` in production code
2. **NO** dead code — zero unused variables, imports, parameters
3. **NO** incomplete implementations or placeholder returns
4. Validate with `cargo check`, not `cargo run`
5. Explicit error handling — use `?`, `thiserror`, `map_err`
6. Minimize allocations — prefer `&str` over `String`, `Cow` over clone
7. Use `parking_lot::Mutex` (sync) or `tokio::sync::Mutex` (async), not `std::sync::Mutex`

## Key Design Decisions

- Core object is **Session**, not Message
- State machine is single source of truth — no bypassing
- Command → State Transition → Event Emit pattern
- Audio thread and UI thread strictly isolated
- FFI exposes minimal, stable, flat DTOs
- Protocol layer is abstract and transport-agnostic
