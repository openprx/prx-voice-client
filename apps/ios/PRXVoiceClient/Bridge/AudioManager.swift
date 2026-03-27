import AVFoundation

/// Manages iOS audio session for voice recording and playback
class AudioManager {
    static let shared = AudioManager()

    private init() {}

    /// Configure audio session for voice chat
    func configureAudioSession() throws {
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.playAndRecord, mode: .voiceChat, options: [.defaultToSpeaker, .allowBluetooth])
        try session.setActive(true)
    }

    /// Deactivate audio session
    func deactivateAudioSession() {
        try? AVAudioSession.sharedInstance().setActive(false)
    }

    /// Check microphone permission
    func checkMicrophonePermission() async -> Bool {
        return await withCheckedContinuation { continuation in
            AVAudioSession.sharedInstance().requestRecordPermission { granted in
                continuation.resume(returning: granted)
            }
        }
    }

    /// Get current audio route info
    func currentRoute() -> String {
        let route = AVAudioSession.sharedInstance().currentRoute
        let outputs = route.outputs.map { $0.portName }.joined(separator: ", ")
        return outputs.isEmpty ? "Unknown" : outputs
    }
}
