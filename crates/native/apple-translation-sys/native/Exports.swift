// The exported C functions (see prxvc_apple_translation.h). Thin: argument checks, the mapping of
// results to codes and messages, and ownership of the session and of returned strings.

import Foundation

private let availabilityTimeoutMs: Int32 = 10_000

private func string(_ p: UnsafePointer<CChar>?) -> String? {
    p.map { String(cString: $0) }
}

private func report(_ code: Int32, _ message: String, _ msg: UnsafeMutablePointer<CChar>?, _ cap: Int) -> Int32 {
    writeMessage(message, msg, cap)
    return code
}

@_cdecl("prxat_abi_version")
public func prxat_abi_version() -> Int32 { 1 }

@_cdecl("prxat_os_support")
public func prxat_os_support() -> Int32 { osSupported() ? PRXAT_OK : PRXAT_OS_TOO_OLD }

@_cdecl("prxat_availability")
public func prxat_availability(_ source: UnsafePointer<CChar>?, _ target: UnsafePointer<CChar>?,
                               _ msg: UnsafeMutablePointer<CChar>?, _ cap: Int) -> Int32 {
    guard let s = string(source), let t = string(target), !s.isEmpty, !t.isEmpty else {
        return report(PRXAT_INVALID_ARGUMENT, "source and target language are required", msg, cap)
    }
    let (code, text) = availabilityCode(s, t, timeoutMs: availabilityTimeoutMs)
    return report(code, text, msg, cap)
}

@_cdecl("prxat_create")
public func prxat_create(_ source: UnsafePointer<CChar>?, _ target: UnsafePointer<CChar>?,
                         _ strategy: Int32, _ code: UnsafeMutablePointer<Int32>?,
                         _ msg: UnsafeMutablePointer<CChar>?, _ cap: Int) -> OpaquePointer? {
    func fail(_ c: Int32, _ m: String) -> OpaquePointer? {
        code?.pointee = report(c, m, msg, cap)
        return nil
    }
    guard let s = string(source), let t = string(target), !s.isEmpty, !t.isEmpty else {
        return fail(PRXAT_INVALID_ARGUMENT, "source and target language are required")
    }
    let (status, text) = availabilityCode(s, t, timeoutMs: availabilityTimeoutMs)
    if status != PRXAT_OK { return fail(status, text) }
    guard let session = Session(source: s, target: t, strategy: strategy) else {
        return fail(PRXAT_OS_TOO_OLD, "System translation needs macOS 26 or later")
    }
    code?.pointee = PRXAT_OK
    return OpaquePointer(Unmanaged.passRetained(session).toOpaque())
}

private func session(_ p: OpaquePointer?) -> Session? {
    p.map { Unmanaged<Session>.fromOpaque(UnsafeRawPointer($0)).takeUnretainedValue() }
}

@_cdecl("prxat_prepare")
public func prxat_prepare(_ handle: OpaquePointer?, _ timeoutMs: Int32,
                          _ msg: UnsafeMutablePointer<CChar>?, _ cap: Int) -> Int32 {
    guard let s = session(handle) else { return report(PRXAT_INVALID_ARGUMENT, "no session", msg, cap) }
    switch s.prepare(timeoutMs: timeoutMs) {
    case .success: return report(PRXAT_OK, "", msg, cap)
    case .failure(let f): return report(f.code, f.message, msg, cap)
    }
}

@_cdecl("prxat_translate")
public func prxat_translate(_ handle: OpaquePointer?, _ text: UnsafePointer<CChar>?, _ timeoutMs: Int32,
                            _ out: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
                            _ msg: UnsafeMutablePointer<CChar>?, _ cap: Int) -> Int32 {
    out?.pointee = nil
    guard let s = session(handle), let input = string(text), let out else {
        return report(PRXAT_INVALID_ARGUMENT, "session, text and output are required", msg, cap)
    }
    switch s.translate(input, timeoutMs: timeoutMs) {
    case .success(let translated):
        out.pointee = strdup(translated)
        return report(PRXAT_OK, "", msg, cap)
    case .failure(let f):
        return report(f.code, f.message, msg, cap)
    }
}

@_cdecl("prxat_cancel")
public func prxat_cancel(_ handle: OpaquePointer?) { session(handle)?.cancel() }

@_cdecl("prxat_destroy")
public func prxat_destroy(_ handle: OpaquePointer?) {
    guard let handle else { return }
    Unmanaged<Session>.fromOpaque(UnsafeRawPointer(handle)).release()
}

@_cdecl("prxat_free")
public func prxat_free(_ text: UnsafeMutablePointer<CChar>?) { free(text) }

@_cdecl("prxat_run_main_loop")
public func prxat_run_main_loop() { CFRunLoopRun() }

@_cdecl("prxat_stop_main_loop")
public func prxat_stop_main_loop() { CFRunLoopStop(CFRunLoopGetMain()) }
