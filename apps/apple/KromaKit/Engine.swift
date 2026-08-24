import Foundation

/// Swift's view of the Rust engine.
///
/// The *only* file on this side that touches the C ABI. Everything above it
/// works in Swift types and never sees a pointer, which is the same firewall
/// the Windows shell has: read the stack, mutate a parameter, ask for a frame,
/// draw it. No image processing on this side of the line.
public enum Engine {
    /// Engine version, for the About panel and for bug reports.
    public static var version: String {
        guard let c = pe_version() else { return "unknown" }
        return String(cString: c)
    }
}

/// What the engine said when it refused.
///
/// The message comes from `pe_session_last_error`. A null handle is the one
/// failure with no message, because there is no session to have recorded one.
public struct EngineError: Error, CustomStringConvertible {
    public let code: Int32
    public let message: String
    public var description: String { message }
}

/// A session owned by the Rust engine.
///
/// A class, not a struct, so `deinit` reliably releases the Rust allocation.
/// Every handle handed across the boundary has exactly one owner on this side.
public final class Session {
    let handle: OpaquePointer

    public init?() {
        guard let h = pe_session_new() else { return nil }
        handle = h
    }

    deinit {
        pe_session_free(handle)
    }

    /// Turn a status code into a thrown error carrying the engine's own words.
    func check(_ code: Int32) throws {
        guard code != 0 else { return }
        throw EngineError(code: code, message: lastError ?? "no reason given")
    }

    /// The last failure's message. Nil when the last call succeeded, and also
    /// when the handle was null — see the note on the status convention.
    var lastError: String? {
        guard let raw = pe_session_last_error(handle) else {
            return nil
        }
        defer { pe_string_free(raw) }
        return String(cString: raw)
    }

    public var rowCount: Int {
        Int(pe_session_row_count(handle))
    }

    public func openTestChart(width: UInt32, height: UInt32) throws {
        try check(pe_session_open_test_chart(handle, width, height))
    }
}
