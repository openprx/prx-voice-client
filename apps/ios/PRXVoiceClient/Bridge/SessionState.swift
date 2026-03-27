import Foundation

/// Observable session state for SwiftUI
@MainActor
class SessionStateManager: ObservableObject {
    @Published var currentState: String = "Idle"
    @Published var isActive: Bool = false
    @Published var sessionId: String?
    @Published var transcriptLines: [TranscriptLine] = []
    @Published var errorMessage: String?

    private let bridge = RustBridge.shared
    private var pollTimer: Timer?

    struct TranscriptLine: Identifiable {
        let id = UUID()
        let role: String  // "user" or "assistant"
        let text: String
        let isFinal: Bool
    }

    func initialize() {
        let result = bridge.initialize()
        switch result {
        case .success:
            currentState = "Ready"
        case .failure(let error):
            errorMessage = error.localizedDescription
        }
    }

    func startSession() {
        let createResult = bridge.createSession()
        switch createResult {
        case .success(let sid):
            sessionId = sid
            let _ = bridge.startSession(sessionId: sid)
            isActive = true
            currentState = "Listening"
            startPolling()
        case .failure(let error):
            errorMessage = error.localizedDescription
        }
    }

    func stopSession() {
        guard let sid = sessionId else { return }
        let _ = bridge.stopSession(sessionId: sid)
        isActive = false
        currentState = "Completed"
        stopPolling()
        sessionId = nil
        transcriptLines = []
    }

    func interrupt() {
        guard let sid = sessionId else { return }
        let _ = bridge.interrupt(sessionId: sid)
    }

    func startPolling() {
        pollTimer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.processEvents()
            }
        }
    }

    func stopPolling() {
        pollTimer?.invalidate()
        pollTimer = nil
    }

    private func processEvents() {
        let events = bridge.pollEvents()
        for event in events {
            guard let eventType = event["event_type"] as? String else { continue }

            switch eventType {
            case "SessionStateChanged":
                if let to = event["to"] as? String {
                    currentState = to
                    isActive = (to != "Completed" && to != "Failed")
                }
            case "InterimTranscriptProduced":
                if let text = event["text"] as? String {
                    transcriptLines.append(TranscriptLine(role: "user", text: text, isFinal: false))
                }
            case "FinalTranscriptProduced":
                if let text = event["text"] as? String {
                    transcriptLines.append(TranscriptLine(role: "user", text: text, isFinal: true))
                }
            case "AssistantReplyTextDelta":
                if let delta = event["delta"] as? String {
                    transcriptLines.append(TranscriptLine(role: "assistant", text: delta, isFinal: true))
                }
            default:
                break
            }
        }
    }
}
