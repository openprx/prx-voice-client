// Bridging of Swift async work to a blocking call with a timeout and cancellation, plus the
// result codes and string helpers of the C ABI (see prxvc_apple_translation.h).

import Foundation

let PRXAT_OK: Int32 = 0
let PRXAT_OS_TOO_OLD: Int32 = 1
let PRXAT_PACK_NOT_INSTALLED: Int32 = 2
let PRXAT_PAIR_UNSUPPORTED: Int32 = 3
let PRXAT_TIMEOUT: Int32 = 4
let PRXAT_CANCELLED: Int32 = 5
let PRXAT_FRAMEWORK_ERROR: Int32 = 6
let PRXAT_INVALID_ARGUMENT: Int32 = 7
let PRXAT_NO_MAIN_LOOP: Int32 = 8

/// A failure with its code and a message for the log and the user.
struct Failure: Error {
    let code: Int32
    let message: String
}

/// The outcome slot shared between the waiting thread and the task. First writer wins.
final class Outcome<T>: @unchecked Sendable {
    private let lock = NSLock()
    private let done = DispatchSemaphore(value: 0)
    private var value: Result<T, Failure>?
    private var task: Task<Void, Never>?

    func set(_ result: Result<T, Failure>) {
        lock.lock()
        let first = value == nil
        if first { value = result }
        lock.unlock()
        if first { done.signal() }
    }

    func attach(_ task: Task<Void, Never>) {
        lock.lock()
        self.task = task
        lock.unlock()
    }

    /// Make the waiting call return `.cancelled` and stop the task.
    func cancel() {
        set(.failure(Failure(code: PRXAT_CANCELLED, message: "cancelled")))
        lock.lock()
        let t = task
        lock.unlock()
        t?.cancel()
    }

    func wait(timeoutMs: Int32) -> Result<T, Failure> {
        let limit: DispatchTime = timeoutMs > 0 ? .now() + .milliseconds(Int(timeoutMs)) : .distantFuture
        if done.wait(timeout: limit) == .timedOut {
            set(.failure(Failure(code: PRXAT_TIMEOUT, message: "no answer within \(timeoutMs) ms")))
        }
        lock.lock()
        let v = value!
        let t = task
        lock.unlock()
        if case .failure = v { t?.cancel() }
        return v
    }
}

/// Run `body` as a task and wait for it on the calling thread. `track` receives the outcome so the
/// session can cancel it from another thread.
func runBlocking<T>(timeoutMs: Int32, track: (Outcome<T>) -> Void = { _ in },
                    _ body: @escaping @Sendable () async throws -> T) -> Result<T, Failure> {
    let outcome = Outcome<T>()
    track(outcome)
    let task = Task.detached {
        do { outcome.set(.success(try await body())) }
        catch let f as Failure { outcome.set(.failure(f)) }
        catch is CancellationError { outcome.set(.failure(Failure(code: PRXAT_CANCELLED, message: "cancelled"))) }
        catch { outcome.set(.failure(Failure(code: PRXAT_FRAMEWORK_ERROR, message: String(describing: error)))) }
    }
    outcome.attach(task)
    return outcome.wait(timeoutMs: timeoutMs)
}

/// Copy `text` into the caller's buffer, truncated at a character boundary, always NUL terminated.
func writeMessage(_ text: String, _ buffer: UnsafeMutablePointer<CChar>?, _ capacity: Int) {
    guard let buffer, capacity > 0 else { return }
    var bytes = Array(text.utf8)
    if bytes.count > capacity - 1 {
        bytes = Array(bytes.prefix(capacity - 1))
        while let last = bytes.last, last & 0xC0 == 0x80 { bytes.removeLast() }
        if let last = bytes.last, last >= 0xC0 { bytes.removeLast() }
    }
    for (i, b) in bytes.enumerated() { buffer[i] = CChar(bitPattern: b) }
    buffer[bytes.count] = 0
}

/// Whether the main dispatch queue is serviced. The framework delivers its work through it, so a
/// process whose main thread does not run its run loop would wait forever.
func checkMainQueue(timeoutMs: Int32 = 1_000) -> Failure? {
    if Thread.isMainThread {
        return Failure(code: PRXAT_NO_MAIN_LOOP,
                       message: "a blocking call was made on the main thread, which the framework needs")
    }
    let served = DispatchSemaphore(value: 0)
    DispatchQueue.main.async { served.signal() }
    if served.wait(timeout: .now() + .milliseconds(Int(timeoutMs))) == .timedOut {
        return Failure(code: PRXAT_NO_MAIN_LOOP,
                       message: "the main thread does not run its run loop, which the framework needs")
    }
    return nil
}
