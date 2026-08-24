import Foundation
import QuartzCore

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

    // ---- opening --------------------------------------------------------

    /// Where this host keeps the application's own files.
    ///
    /// The engine does not guess. On a Mac the answer is Application Support;
    /// nothing in Rust can know that, and the one build that guessed wrote to
    /// `~/.config` for years without anybody noticing.
    public func setSupportDirectory(_ url: URL) throws {
        try check(url.path.withCString {
            pe_session_set_support_dir(handle, $0)
        })
    }

    public func open(_ url: URL) throws {
        try check(url.path.withCString {
            pe_session_open_path(handle, $0)
        })
    }

    // ---- the screen -----------------------------------------------------

    /// Hand the engine a layer to draw into.
    ///
    /// The layer stays owned by the view. `detachLayer()` must be called
    /// before that view goes away.
    public func attach(layer: CALayer, width: UInt32, height: UInt32) throws {
        let raw = Unmanaged.passUnretained(layer).toOpaque()
        try check(pe_session_attach_layer(handle, raw, width, height))
    }

    public func resize(width: UInt32, height: UInt32) throws {
        try check(pe_session_resize(handle, width, height))
    }

    public func detachLayer() {
        _ = pe_session_detach_layer(handle)
    }

    public func render() throws {
        try check(pe_session_render(handle))
    }

    public var needsRender: Bool {
        pe_session_needs_render(handle)
    }

    /// Passes the last frame executed. The number that proves the stage cache
    /// works: dragging one slider on a deep stack should read 1, not the depth.
    public var lastPasses: Int {
        Int(pe_session_last_passes(handle))
    }

    /// Drives the autosave debounce. Called from the display link.
    /// Throws, because the tick is what drives the autosave debounce and a
    /// failed autosave is the one thing here worth hearing about. Discarding it
    /// is how the same failure stayed invisible on the Windows side for as long
    /// as it did.
    public func tick() throws {
        try check(pe_session_tick(handle))
    }

    // ---- the document -----------------------------------------------------

    public var snapshotVersion: UInt64 {
        pe_session_snapshot_version(handle)
    }

    /// The whole UI-visible state, decoded.
    public func snapshot() throws -> Snapshot {
        guard let raw = pe_session_snapshot_json(handle) else {
            throw EngineError(code: -1, message: "the engine produced no snapshot")
        }
        defer { pe_string_free(raw) }
        let data = Data(String(cString: raw).utf8)
        return try JSONDecoder().decode(Snapshot.self, from: data)
    }

    @discardableResult
    public func addEffect(_ key: String) throws -> UInt64 {
        let id = key.withCString { pe_session_add_effect(handle, $0) }
        guard id >= 0 else {
            throw EngineError(code: Int32(id), message: lastError ?? "no reason given")
        }
        return UInt64(id)
    }

    public func removeRow(_ row: UInt64) throws {
        try check(pe_session_remove_row(handle, row))
    }

    public func moveRow(_ row: UInt64, to index: UInt32) throws {
        try check(pe_session_move_row(handle, row, index))
    }

    public func setRowEnabled(_ row: UInt64, _ on: Bool) throws {
        try check(pe_session_set_row_enabled(handle, row, on))
    }

    public func setRowOpacity(_ row: UInt64, _ value: Float) throws {
        try check(pe_session_set_row_opacity(handle, row, value))
    }

    // ---- parameters, the hot path -----------------------------------------

    public func setFloat(row: UInt64, key: String, value: Float) throws {
        try check(key.withCString {
            pe_session_set_float(handle, row, $0, value)
        })
    }

    public func setBool(row: UInt64, key: String, value: Bool) throws {
        try check(key.withCString {
            pe_session_set_bool(handle, row, $0, value)
        })
    }

    public func setChoice(row: UInt64, key: String, value: String) throws {
        try check(key.withCString { k in
            value.withCString { v in
                pe_session_set_choice(handle, row, k, v)
            }
        })
    }

    public func setRGB(row: UInt64, key: String, _ r: Float, _ g: Float, _ b: Float) throws {
        try check(key.withCString {
            pe_session_set_rgb(handle, row, $0, r, g, b)
        })
    }

    public func setWheel(
        row: UInt64, key: String, master: Float, _ r: Float, _ g: Float, _ b: Float
    ) throws {
        try check(key.withCString {
            pe_session_set_wheel(handle, row, $0, master, r, g, b)
        })
    }

    // ---- history ------------------------------------------------------------

    /// Bracket a drag so it collapses into one undo step rather than three
    /// hundred. Not throwing: a failure here would mean a drag that cannot be
    /// started, and there is nothing useful for a slider to do about that.
    public func beginInteraction(_ label: String) {
        _ = label.withCString {
            pe_session_begin_interaction(handle, $0)
        }
    }

    public func endInteraction() {
        _ = pe_session_end_interaction(handle)
    }

    public var canUndo: Bool { pe_session_can_undo(handle) }
    public var canRedo: Bool { pe_session_can_redo(handle) }

    /// True when it moved, false when there was nothing to undo.
    @discardableResult
    public func undo() throws -> Bool {
        let moved = pe_session_undo(handle)
        guard moved >= 0 else {
            throw EngineError(code: moved, message: lastError ?? "no reason given")
        }
        return moved == 1
    }

    @discardableResult
    public func redo() throws -> Bool {
        let moved = pe_session_redo(handle)
        guard moved >= 0 else {
            throw EngineError(code: moved, message: lastError ?? "no reason given")
        }
        return moved == 1
    }

    // ---- persistence and export ---------------------------------------------

    @discardableResult
    public func saveSidecar() throws -> URL {
        guard let raw = pe_session_save_sidecar(handle) else {
            throw EngineError(code: -1, message: lastError ?? "the sidecar was not written")
        }
        defer { pe_string_free(raw) }
        return URL(fileURLWithPath: String(cString: raw))
    }

    public func loadSidecar(_ url: URL) throws {
        try check(url.path.withCString {
            pe_session_load_sidecar(handle, $0)
        })
    }

    public func revert() throws {
        try check(pe_session_revert(handle))
    }

    /// Write the work in progress now, throttle or no throttle.
    ///
    /// Called when leaving a photograph or closing the window, where waiting
    /// for the debounce would mean waiting for something that is about to stop
    /// happening. The tick will not do: it respects the debounce.
    public func flushAutosave() throws {
        try check(pe_session_flush_autosave(handle))
    }

    public func setExport(format: String, quality: UInt8) throws {
        try check(format.withCString {
            pe_session_set_export(handle, $0, quality)
        })
    }

    /// Export beside the original, returning where it went.
    ///
    /// Throws when the engine refuses — which it does when the output would
    /// land on one of the photographs it was given. That refusal is the point,
    /// not an error condition to work around.
    @discardableResult
    public func export() throws -> URL {
        guard let raw = pe_session_export(handle) else {
            throw EngineError(code: -1, message: lastError ?? "the export was not written")
        }
        defer { pe_string_free(raw) }
        return URL(fileURLWithPath: String(cString: raw))
    }
}

extension Engine {
    /// Every effect and every parameter, decoded once at launch.
    ///
    /// The whole inspector is generated from this. Adding an effect in Rust
    /// makes it appear here with no Swift changes at all.
    public static func registry() throws -> Registry {
        guard let raw = pe_registry_json() else {
            throw EngineError(code: -1, message: "the engine produced no registry")
        }
        defer { pe_string_free(raw) }
        let data = Data(String(cString: raw).utf8)
        return try JSONDecoder().decode(Registry.self, from: data)
    }
}
