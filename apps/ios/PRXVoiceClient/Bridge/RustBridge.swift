import Foundation

/// Swift wrapper around the Rust FFI C API
final class RustBridge {
    static let shared = RustBridge()

    private init() {}

    /// Initialize the Rust engine
    func initialize(configJson: String = "{}") -> Result<[String: Any], RustBridgeError> {
        return callRust { prxvc_initialize(configJson) }
    }

    /// Shutdown the engine
    func shutdown() {
        prxvc_shutdown()
    }

    /// Create a new session
    func createSession(configJson: String = "{}") -> Result<String, RustBridgeError> {
        let result = callRust { prxvc_create_session(configJson) }
        switch result {
        case .success(let dict):
            if let sessionId = dict["session_id"] as? String {
                return .success(sessionId)
            }
            return .failure(.invalidResponse)
        case .failure(let error):
            return .failure(error)
        }
    }

    /// Start a session
    func startSession(sessionId: String) -> Result<[String: Any], RustBridgeError> {
        return callRust { prxvc_start_session(sessionId) }
    }

    /// Stop a session
    func stopSession(sessionId: String) -> Result<[String: Any], RustBridgeError> {
        return callRust { prxvc_stop_session(sessionId) }
    }

    /// Interrupt playback
    func interrupt(sessionId: String) -> Result<[String: Any], RustBridgeError> {
        return callRust { prxvc_interrupt(sessionId) }
    }

    /// Poll for events from the engine
    func pollEvents() -> [[String: Any]] {
        guard let ptr = prxvc_poll_events() else { return [] }
        defer { prxvc_free_string(UnsafeMutablePointer(mutating: ptr)) }

        let str = String(cString: ptr)
        guard let data = str.data(using: .utf8),
              let array = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
            return []
        }
        return array
    }

    /// Get session state
    func getSessionState(sessionId: String) -> Result<[String: Any], RustBridgeError> {
        return callRust { prxvc_get_state(sessionId) }
    }

    // MARK: - Private helpers

    private func callRust(_ call: @autoclosure () -> UnsafePointer<CChar>?) -> Result<[String: Any], RustBridgeError> {
        guard let ptr = call() else {
            return .failure(.nullResponse)
        }
        defer { prxvc_free_string(UnsafeMutablePointer(mutating: ptr)) }

        let str = String(cString: ptr)
        guard let data = str.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return .failure(.invalidResponse)
        }

        if let error = json["error"] as? String {
            return .failure(.engineError(error))
        }

        return .success(json)
    }
}

enum RustBridgeError: Error, LocalizedError {
    case nullResponse
    case invalidResponse
    case engineError(String)

    var errorDescription: String? {
        switch self {
        case .nullResponse: return "Null response from engine"
        case .invalidResponse: return "Invalid response format"
        case .engineError(let msg): return msg
        }
    }
}
