# Release Process

## Version Scheme

Following Semantic Versioning (SemVer):

| Range | Stage |
|-------|-------|
| 0.1.x | Internal prototype |
| 0.2.x | Core flows working |
| 0.3.x | External testing |
| 0.5.x | Public beta |
| 1.0.0 | Stable release |

## Release Checklist

1. All CI checks pass on `main`
2. `cargo test --workspace` passes
3. `cargo clippy --workspace -- -D warnings` has no warnings
4. CHANGELOG.md updated with new version section
5. Version bumped in workspace `Cargo.toml`
6. Create and push git tag: `git tag v0.1.0 && git push origin v0.1.0`
7. GitHub Actions builds and publishes release artifacts (the macOS job runs
   `scripts/package-macos.sh --zip`; see "macOS application bundle" below)
8. Write release notes on GitHub

## Artifacts

| Platform | Artifact |
|----------|----------|
| macOS (arm64) | `prx-voice-client-macos-arm64.zip`: `PRX Voice.app`, ad-hoc signed (see below) |
| Linux (x86_64) | `prx-voice-client-linux-x86_64.tar.gz` |
| Windows (x86_64) | `prx-voice-client-windows-x86_64.zip` |

The Linux and Windows archives hold the bare `prxvc-desktop` binary. The iOS
(`libprxvc_ffi.a`) and Android (`libprxvc_ffi.so`) libraries are not built or
published by any workflow yet.

## macOS application bundle

`scripts/package-macos.sh` builds `PRX Voice.app` (bundle identifier
`org.openprx.prx-voice-client`, version from the workspace `Cargo.toml`) into
`dist/`. It signs ad-hoc unless `PRXVC_CODESIGN_IDENTITY` names a Developer ID
Application identity, in which case it uses the hardened runtime and
`packaging/macos/entitlements.plist`. The release workflow has no certificate,
so the published zip is ad-hoc signed and **not notarized**: macOS Gatekeeper
rejects a downloaded copy until the user allows it (System Settings, Privacy &
Security, "Open Anyway"). Signed and notarized releases need a Developer ID
certificate and notarytool credentials, supplied by the owner; the commands are
in `packaging/macos/README.md`. No installer or disk image is produced.

## Hotfix Process

1. Branch from the release tag
2. Fix, test, review
3. Tag as next patch version
4. Merge back to `main`
