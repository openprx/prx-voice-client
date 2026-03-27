# PRX Voice Client -- Android

Native Android app using Kotlin/Compose with shared Rust Core via JNI/FFI.

## Prerequisites

- Android Studio Hedgehog (2023.1+) or later
- Android SDK API 26+ (minSdk)
- Android NDK r25+ (set `ANDROID_NDK_HOME`)
- Rust toolchain (rustup) with Android targets:
  ```bash
  rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
  ```
- cargo-ndk:
  ```bash
  cargo install cargo-ndk
  ```

## Architecture

```
MainActivity (Compose)
    |
    +-- MainNavigation (bottom tabs)
    |       +-- SessionScreen    (voice session UI)
    |       +-- HistoryScreen    (past sessions)
    |       +-- SettingsScreen   (preferences)
    |
    +-- SessionStateManager (state + coroutine polling)
    |       |
    |       +-- RustBridge (JNI -> libprxvc_ffi.so)
    |               |
    |               +-- prxvc-ffi (Rust C API)
    |                       |
    |                       +-- prxvc-session, prxvc-audio, etc.
    |
    +-- AudioSessionManager (AudioFocus, device routing)
```

### JNI Bridge

`RustBridge.kt` loads `libprxvc_ffi.so` via `System.loadLibrary("prxvc_ffi")` and
declares `external` functions that map to the C API exported by the `prxvc-ffi` crate.
All data crosses the JNI boundary as JSON strings for simplicity and forward
compatibility. The Kotlin side parses responses into `JSONObject` and wraps errors
in `Result<T>`.

### State Management

`SessionStateManager` holds Compose `mutableStateOf` fields and polls events from
the Rust engine on a coroutine (100ms interval). UI screens observe these states
reactively via Compose delegation (`by sessionState.currentState`).

## Build

### 1. Build Rust native libraries

```bash
cd apps/android
./build-rust.sh
```

This produces `.so` files under `app/src/main/jniLibs/{arm64-v8a,armeabi-v7a,x86_64}/`.

### 2. Open in Android Studio

Open `apps/android/` as an Android project. Gradle will pick up the JNI libraries
from `jniLibs/`.

### 3. Run

Select a device or emulator and run from Android Studio, or:

```bash
./gradlew installDebug
```

## Project Structure

```
apps/android/
  app/src/main/
    AndroidManifest.xml
    kotlin/com/prx/voiceclient/
      PRXVoiceApp.kt          -- Application class
      MainActivity.kt         -- Entry point, hosts Compose
      bridge/
        RustBridge.kt          -- JNI native method declarations
        SessionStateManager.kt -- Reactive state + event polling
      ui/
        Navigation.kt          -- Bottom tab navigation
        SessionScreen.kt       -- Voice session controls + transcript
        HistoryScreen.kt       -- Session history list
        SettingsScreen.kt      -- App preferences
        PermissionScreen.kt    -- Microphone permission request
      audio/
        AudioManager.kt        -- AudioFocus and device routing
      model/
        Session.kt             -- VoiceSession data class
        AppState.kt            -- App lifecycle enum
    res/values/
      strings.xml
  build-rust.sh                -- Cross-compile Rust for Android
```

## Permissions

| Permission | Purpose |
|---|---|
| RECORD_AUDIO | Voice input capture |
| BLUETOOTH / BLUETOOTH_CONNECT | Bluetooth headset routing |
| FOREGROUND_SERVICE / FOREGROUND_SERVICE_MICROPHONE | Keep session alive in background |
| INTERNET | Network communication with AI services |

## Responsibilities

- AudioFocus management
- Foreground Service for active sessions
- Bluetooth device change handling
- Notification bar controls
- Background recovery strategy
- Device compatibility matrix
