# Xcode Project

This directory will contain the Xcode project file.

To set up:
1. Open Xcode -> Create new project -> iOS App -> SwiftUI
2. Set product name to "PRXVoiceClient"
3. Set bundle identifier to "com.prx.voice-client"
4. Copy Swift source files from this directory
5. Add bridging header: PRXVoiceClient-Bridging-Header.h
6. Link libprxvc_ffi.a (built via build-rust.sh)
7. Add Library Search Paths to the Rust target directory

See the project README for detailed setup instructions.
