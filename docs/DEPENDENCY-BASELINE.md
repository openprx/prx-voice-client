# Dependency Baseline

Generated: 2026-03-27
Rust Edition: 2024
Rust Toolchain: stable

## Direct Dependencies (depth 1)

```
prxvc-audio v0.1.0
├── cpal v0.15.3
├── parking_lot v0.12.5
├── prxvc-core v0.1.0
├── prxvc-protocol v0.1.0
├── serde v1.0.228
├── thiserror v2.0.18
├── tracing v0.1.44
└── uuid v1.23.0

prxvc-config v0.1.0
├── directories v6.0.0
├── serde v1.0.228
├── thiserror v2.0.18
├── toml v0.8.2
└── tracing v0.1.44

prxvc-core v0.1.0

prxvc-desktop v0.1.0
├── chrono v0.4.44
├── directories v6.0.0
├── eframe v0.31.1
├── egui v0.31.1
├── global-hotkey v0.6.4
├── parking_lot v0.12.5
├── prxvc-config v0.1.0
├── prxvc-core v0.1.0
├── prxvc-desktop-ui v0.1.0
├── prxvc-i18n v0.1.0
├── prxvc-observe v0.1.0
├── prxvc-protocol v0.1.0
├── prxvc-session v0.1.0
├── prxvc-storage v0.1.0
├── tokio v1.50.0
├── tracing v0.1.44
├── tracing-subscriber v0.3.23
├── tray-icon v0.19.3
└── uuid v1.23.0

prxvc-desktop-ui v0.1.0

prxvc-ffi v0.1.0
├── chrono v0.4.44
├── parking_lot v0.12.5
├── prxvc-config v0.1.0
├── prxvc-core v0.1.0
├── prxvc-protocol v0.1.0
├── prxvc-session v0.1.0
├── serde v1.0.228
├── serde_json v1.0.149
├── thiserror v2.0.18
├── tracing v0.1.44
└── uuid v1.23.0

prxvc-i18n v0.1.0

prxvc-observe v0.1.0

prxvc-protocol v0.1.0

prxvc-security v0.1.0
├── serde v1.0.228
├── thiserror v2.0.18
└── tracing v0.1.44

prxvc-session v0.1.0

prxvc-storage v0.1.0

prxvc-testkit v0.1.0
├── chrono v0.4.44
├── prxvc-audio v0.1.0
├── prxvc-core v0.1.0
├── prxvc-protocol v0.1.0
├── prxvc-session v0.1.0
├── serde_json v1.0.149
└── uuid v1.23.0
```

## Upgrade Policy

- Direct dependencies are reviewed monthly
- Security patches applied immediately
- Platform-sensitive deps (cpal, egui, tungstenite) undergo gray-scale upgrade
- Audio regression suite must pass before any audio-related dep upgrade
