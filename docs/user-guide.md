# User Guide

## Quick Start

### Desktop

1. Launch the application (`prxvc-desktop`).
2. Click **Start** or press `Ctrl+Shift+Space` to begin a voice session.
3. Speak naturally into your microphone.
4. View the real-time transcript as it appears.
5. Click **Stop** or press the hotkey again to end the session.

## Features

### Voice Session

A voice session is a multi-turn conversation between you and an AI assistant.

- **Start/Stop** with the on-screen button or the global hotkey.
- **Real-time transcript** displays your speech and the assistant's replies as they happen.
- **Interrupt** the AI at any time by clicking the Interrupt button while the assistant is speaking.
- **Multi-turn** conversations carry context across exchanges within a single session.

### Session States

The session indicator (colored dot) shows what the system is currently doing:

| State | Color | Description |
|-------|-------|-------------|
| Idle | Gray | No active session. Ready to start. |
| Starting | -- | Session is initializing and connecting. |
| Listening | Green | Microphone is active, waiting for you to speak. |
| User Speaking | Green | Your speech is being captured. |
| Recognizing | Yellow | Speech-to-text is processing your audio. |
| Thinking | Yellow | The AI assistant is generating a reply. |
| Assistant Speaking | Blue | The assistant's audio response is playing. |
| Interrupted | -- | You interrupted the assistant. Returning to listening. |
| Paused | -- | Session is temporarily paused. |
| Recovering | Orange | A transient error occurred. Auto-recovering. |
| Completed | Gray | Session ended normally. |
| Failed | Red | Session terminated due to an error. |

### History

Navigate to the **History** page to browse past sessions. Each entry shows:

- Session title (auto-generated summary)
- Date and time
- Duration
- Final status (Completed, Failed, etc.)

History is stored locally in a SQLite database and persists across app restarts.

### Settings

Navigate to the **Settings** page to configure:

| Setting | Description |
|---------|-------------|
| Language | Switch between English and Chinese (Simplified). |
| Input Device | Select which microphone to use. |
| Output Device | Select which speaker/headphone to use. |
| Auto-play Replies | Automatically play assistant audio responses. |
| Save History | Persist session records to local storage. |
| Show Transcript | Display live transcript during sessions. |
| Hotkey Toggle | Enable or disable the global hotkey. |

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+Space` | Start a new session / Stop the current session |

The hotkey works globally (even when the app is in the background) when enabled in Settings.

### System Tray

The application adds an icon to the system tray with the following actions:

| Action | Description |
|--------|-------------|
| Show | Bring the application window to the foreground. |
| Start Session | Begin a new voice session. |
| Stop Session | End the current session. |
| Settings | Open the Settings page. |
| Quit | Close the application. |

### Diagnostics

The **Debug** page provides developer-oriented information:

- **State Machine** -- shows the current session state and session ID.
- **Event Log** -- a scrollable list of all engine events (most recent first).
- **Pending Commands** -- number of queued commands awaiting dispatch.

You can export a diagnostics bundle from the debug page for troubleshooting. The bundle is saved as a JSON file in your system's temporary directory.

## Troubleshooting

### No audio input detected

1. Check that your operating system has granted microphone permission to the application.
2. Open **Settings** and verify the correct input device is selected.
3. Ensure the microphone is not in use by another application.
4. On Linux, confirm ALSA is installed and your user has audio group membership.

### Connection issues

- The app automatically attempts to reconnect up to 5 times with exponential backoff.
- While reconnecting, the network indicator in the navigation bar shows an orange icon.
- If all reconnect attempts fail, a red error banner appears. Click **Retry** to manually attempt reconnection.
- Check your network connectivity if the problem persists.

### Session stuck in a state

- Click **Stop** to end the current session, then start a new one.
- If Stop does not respond, restart the application.

### App won't start

1. Verify you meet the system requirements (Rust 1.85+, platform audio libraries).
2. Try deleting the configuration directory to reset to defaults:
   - **macOS:** `~/Library/Application Support/com.prx.prx-voice-client/`
   - **Linux:** `~/.config/prx/prx-voice-client/`
   - **Windows:** `%APPDATA%\prx\prx-voice-client\`
3. Run the app from the terminal to see error output:
   ```bash
   RUST_LOG=debug prxvc-desktop
   ```

### Audio quality issues

- The default sample rate is 16 kHz mono, optimized for voice.
- Echo control is enabled by default. If you experience echo, check that your audio device supports echo cancellation.
- Adjust input/output gain through your operating system's audio settings.
