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
7. GitHub Actions builds and publishes release artifacts
8. Write release notes on GitHub

## Artifacts

| Platform | Artifact |
|----------|----------|
| macOS (arm64) | `prx-voice-client-macos-arm64.tar.gz` |
| Linux (x86_64) | `prx-voice-client-linux-x86_64.tar.gz` |
| Windows (x86_64) | `prx-voice-client-windows-x86_64.zip` |
| iOS | `libprxvc_ffi.a` (static library for Xcode) |
| Android | `libprxvc_ffi.so` (shared library for Gradle) |

## Hotfix Process

1. Branch from the release tag
2. Fix, test, review
3. Tag as next patch version
4. Merge back to `main`
