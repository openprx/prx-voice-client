# PRX Voice Client — iOS

Native iOS app using Swift/SwiftUI with shared Rust Core via C FFI.

## Status

Skeleton project with Swift bridge code calling the Rust FFI layer (`prxvc-ffi`).

## Architecture

- **UI**: SwiftUI (iOS 16+) with TabView navigation
- **Rust Bridge**: C FFI bridging header + `RustBridge.swift` wrapper
- **State Management**: `SessionStateManager` (ObservableObject) with event polling
- **Audio**: Native `AVAudioSession` managed by `AudioManager`
- **Permissions**: Native iOS permission flow via `PermissionView`

## Project Structure

```
apps/ios/
├── PRXVoiceClient/
│   ├── App/
│   │   ├── PRXVoiceClientApp.swift    # @main entry point
│   │   └── ContentView.swift          # Tab navigation root
│   ├── Views/
│   │   ├── SessionView.swift          # Live session UI with transcript
│   │   ├── HistoryView.swift          # Past sessions list
│   │   ├── SettingsView.swift         # App preferences
│   │   └── PermissionView.swift       # Microphone permission request
│   ├── Bridge/
│   │   ├── RustBridge.swift           # Swift wrapper for Rust C FFI
│   │   ├── SessionState.swift         # Observable state manager
│   │   └── AudioManager.swift         # AVAudioSession lifecycle
│   ├── Models/
│   │   ├── Session.swift              # VoiceSession data model
│   │   └── AppState.swift             # App lifecycle state enum
│   └── Resources/
│       └── Info.plist                 # Bundle config + mic permission
├── PRXVoiceClient.xcodeproj/          # Xcode project (placeholder)
├── bridging-header.h                  # C FFI function declarations
└── build-rust.sh                      # Build Rust for iOS targets
```

## Build Setup

### Prerequisites

- Xcode 15+ with iOS 16+ SDK
- Rust toolchain with iOS targets:
  ```bash
  rustup target add aarch64-apple-ios aarch64-apple-ios-sim
  ```

### Step 1: Build Rust FFI Library

```bash
cd apps/ios
./build-rust.sh
```

This produces static libraries:
- `target/aarch64-apple-ios-sim/release/libprxvc_ffi.a` (Simulator)
- `target/aarch64-apple-ios/release/libprxvc_ffi.a` (Device)

### Step 2: Set Up Xcode Project

1. Open Xcode and create a new iOS App project (SwiftUI lifecycle)
2. Set product name to `PRXVoiceClient`, bundle ID to `com.prx.voice-client`
3. Add all `.swift` files from the `PRXVoiceClient/` directory
4. Configure bridging header:
   - Build Settings -> Swift Compiler -> Objective-C Bridging Header
   - Set to: `$(SRCROOT)/bridging-header.h`
5. Link the Rust static library:
   - Build Phases -> Link Binary With Libraries -> Add `libprxvc_ffi.a`
6. Add Library Search Paths:
   - For Debug (Simulator): `$(PROJECT_DIR)/../../target/aarch64-apple-ios-sim/debug`
   - For Release (Device): `$(PROJECT_DIR)/../../target/aarch64-apple-ios/release`
7. Add `Info.plist` from `Resources/` to the target

### Step 3: Run

Select a simulator or device target and build (Cmd+R).

## Bridge Layer

The bridge layer translates between Swift and Rust via a JSON-based C FFI:

1. **bridging-header.h** declares the C function signatures
2. **RustBridge.swift** wraps each C call with JSON serialization/deserialization
3. **SessionStateManager** provides SwiftUI-observable state by polling Rust events
4. **AudioManager** handles iOS-specific AVAudioSession configuration

All FFI calls return JSON strings. The Swift side parses responses and maps
errors to `RustBridgeError` for clean Swift error handling.

## Key Design Decisions

- Session-centric architecture matching the Rust core design
- JSON-based FFI for minimal surface area and easy debugging
- Event polling (100ms interval) bridges Rust async events to SwiftUI
- No force unwraps anywhere — all error paths handled gracefully
- AVAudioSession configured for `.voiceChat` mode with Bluetooth support
