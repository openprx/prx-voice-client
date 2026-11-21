// The Translation framework behind the C ABI. Everything that needs macOS 26 sits behind
// `#available` checks, so the library loads and answers PRXAT_OS_TOO_OLD on older systems.

import Foundation
import Translation

@available(macOS 26.0, *)
func languageStatus(_ source: String, _ target: String) async -> LanguageAvailability.Status {
    await LanguageAvailability().status(
        from: Locale.Language(identifier: source), to: Locale.Language(identifier: target))
}

func osSupported() -> Bool {
    if #available(macOS 26.0, *) { return true }
    return false
}

/// The availability of the pair as a result code.
func availabilityCode(_ source: String, _ target: String, timeoutMs: Int32) -> (Int32, String) {
    guard #available(macOS 26.0, *) else {
        return (PRXAT_OS_TOO_OLD, "System translation needs macOS 26 or later")
    }
    if let f = checkMainQueue() { return (f.code, f.message) }
    let r = runBlocking(timeoutMs: timeoutMs) { await languageStatus(source, target) }
    switch r {
    case .failure(let f): return (f.code, f.message)
    case .success(let status):
        switch status {
        case .installed: return (PRXAT_OK, "installed")
        case .supported: return (PRXAT_PACK_NOT_INSTALLED, "the language pack for \(source) to \(target) is not downloaded")
        case .unsupported: return (PRXAT_PAIR_UNSUPPORTED, "\(source) to \(target) is not supported")
        @unknown default: return (PRXAT_FRAMEWORK_ERROR, "unknown availability status")
        }
    }
}

/// A box that lets a non-Sendable framework session cross into the task.
final class SessionBox: @unchecked Sendable {
    let session: Any
    init(_ session: Any) { self.session = session }
}

@available(macOS 26.0, *)
func makeFrameworkSession(_ source: String, _ target: String, _ strategy: Int32) -> SessionBox {
    let s = Locale.Language(identifier: source)
    let t = Locale.Language(identifier: target)
    if strategy != 0, #available(macOS 26.4, *) {
        let st: TranslationSession.Strategy = strategy == 2 ? .highFidelity : .lowLatency
        return SessionBox(TranslationSession(installedSource: s, target: t, preferredStrategy: st))
    }
    return SessionBox(TranslationSession(installedSource: s, target: t))
}

@available(macOS 26.0, *)
func prepareBox(_ box: SessionBox) async throws {
    try await (box.session as! TranslationSession).prepareTranslation()
}

@available(macOS 26.0, *)
func translateBox(_ box: SessionBox, _ text: String) async throws -> String {
    try await (box.session as! TranslationSession).translate(text).targetText
}

/// One session of the C ABI: a framework session, a call lock and the call in flight.
final class Session: @unchecked Sendable {
    let source: String
    let target: String
    let strategy: Int32
    private let callLock = NSLock()   // serialises calls
    private let stateLock = NSLock()  // guards box and current
    private var box: SessionBox
    private var current: AnyObject?
    private var cancelCurrent: (() -> Void)?

    init?(source: String, target: String, strategy: Int32) {
        guard #available(macOS 26.0, *) else { return nil }
        self.source = source
        self.target = target
        self.strategy = strategy
        self.box = makeFrameworkSession(source, target, strategy)
    }

    private func currentBox() -> SessionBox {
        stateLock.lock(); defer { stateLock.unlock() }
        return box
    }

    /// After a timeout the framework session may still be busy with the abandoned call.
    private func renew() {
        guard #available(macOS 26.0, *) else { return }
        let fresh = makeFrameworkSession(source, target, strategy)
        stateLock.lock(); box = fresh; stateLock.unlock()
    }

    func cancel() {
        stateLock.lock(); let c = cancelCurrent; stateLock.unlock()
        c?()
    }

    private func run<T>(timeoutMs: Int32, _ body: @escaping @Sendable (SessionBox) async throws -> T)
        -> Result<T, Failure> {
        callLock.lock(); defer { callLock.unlock() }
        let b = currentBox()
        let r = runBlocking(timeoutMs: timeoutMs, track: { outcome in
            self.stateLock.lock()
            self.cancelCurrent = { outcome.cancel() }
            self.stateLock.unlock()
        }) { try await body(b) }
        stateLock.lock(); cancelCurrent = nil; stateLock.unlock()
        if case .failure(let f) = r, f.code == PRXAT_TIMEOUT { renew() }
        return r
    }

    func prepare(timeoutMs: Int32) -> Result<Void, Failure> {
        guard #available(macOS 26.0, *) else {
            return .failure(Failure(code: PRXAT_OS_TOO_OLD, message: "System translation needs macOS 26 or later"))
        }
        if let f = checkMainQueue() { return .failure(f) }
        return run(timeoutMs: timeoutMs) { try await prepareBox($0) }
    }

    func translate(_ text: String, timeoutMs: Int32) -> Result<String, Failure> {
        guard #available(macOS 26.0, *) else {
            return .failure(Failure(code: PRXAT_OS_TOO_OLD, message: "System translation needs macOS 26 or later"))
        }
        return run(timeoutMs: timeoutMs) { try await translateBox($0, text) }
    }
}
