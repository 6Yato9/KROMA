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

    /// Show this rectangle of the frame. `size` is the fraction of the whole
    /// picture that is visible, so 1 is fitted and 0.25 is four times in.
    public func setView(x: Float, y: Float, size: Float) throws {
        try check(pe_session_set_view(handle, x, y, size))
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

    /// Replace a curve's control points.
    ///
    /// The points cross as a flat array of floats rather than as JSON, because
    /// this is a drag path — the whole curve is sent on every frame, and a
    /// parse per frame to carry twenty numbers is work nobody needs done.
    public func setCurve(row: UInt64, key: String, points: [CGPoint]) throws {
        let xy = points.flatMap { [Float($0.x), Float($0.y)] }
        try check(
            key.withCString { k in
                xy.withUnsafeBufferPointer { buf in
                    pe_session_set_curve(handle, row, k, buf.baseAddress, UInt32(points.count))
                }
            }
        )
    }

    /// Move one vertex of a lattice. The offset is a displacement in axis
    /// units, not a position — that is what a warp stores.
    public func setWarpVertex(
        row: UInt64, key: String, col: Int, vertexRow: Int, offset: CGPoint
    ) throws {
        try check(
            key.withCString {
                pe_session_set_warp_vertex(
                    handle, row, $0, UInt32(col), UInt32(vertexRow),
                    Float(offset.x), Float(offset.y)
                )
            }
        )
    }

    /// Put a lattice back to identity, keeping its grid size.
    public func clearWarp(row: UInt64, key: String) throws {
        try check(key.withCString { pe_session_clear_warp(handle, row, $0) })
    }

    /// Place a pin at a chromaticity, returning its index.
    ///
    /// The odd one out: it answers with an index, so failure arrives in the
    /// same integer as the answer rather than through `check`. `-1` is a bad
    /// argument, which the engine records nothing about because it never got
    /// far enough to have a reason; `-2` is a refusal, whose reason is where
    /// every other refusal's is.
    public func addPin(row: UInt64, key: String, at: CGPoint) throws -> Int {
        let i = key.withCString {
            pe_session_add_pin(handle, row, $0, Float(at.x), Float(at.y))
        }
        guard i >= 0 else {
            throw EngineError(code: i, message: lastError ?? "no reason given")
        }
        return Int(i)
    }

    /// Drag a pin. Only `to` moves — `at` is where the colour is.
    public func movePin(row: UInt64, key: String, index: Int, to: CGPoint) throws {
        try check(key.withCString {
            pe_session_move_pin(handle, row, $0, UInt32(index), Float(to.x), Float(to.y))
        })
    }

    /// The five controls that shape a pin, set together.
    ///
    /// One call rather than five, so a slider drag is one undo step rather
    /// than five parameters racing each other into the history.
    public func setPinShape(
        row: UInt64, key: String, index: Int,
        chromaRange: Double, tonalLow: Double, tonalHigh: Double,
        tonalPivot: Double, exposure: Double
    ) throws {
        try check(key.withCString {
            pe_session_set_pin_shape(
                handle, row, $0, UInt32(index), Float(chromaRange), Float(tonalLow),
                Float(tonalHigh), Float(tonalPivot), Float(exposure)
            )
        })
    }

    public func removePin(row: UInt64, key: String, index: Int) throws {
        try check(key.withCString {
            pe_session_remove_pin(handle, row, $0, UInt32(index))
        })
    }

    // ---- crop, straighten, flips --------------------------------------------

    /// Propose a geometry, and take back the one the engine actually stored.
    ///
    /// **What comes back is frequently not what was passed in, and that is the
    /// point of this call.** The engine corrects: quarter-turns are taken
    /// modulo four, a locked aspect re-shapes the crop, and the crop is then
    /// slid — and, if it still will not fit anywhere, shrunk — back inside the
    /// straightened source. The returned value is what the document now holds,
    /// which is why nothing on this side has a second copy of `apply_aspect`,
    /// `slide_to_fit` and `shrink_to_fit` to keep honest.
    ///
    /// A call site that discards the answer and goes on drawing what it asked
    /// for is drawing a rectangle the engine did not accept, and that rectangle
    /// will jump to the real one the moment the drag ends and the snapshot is
    /// read again. Deliberately not `@discardableResult`, so throwing the
    /// correction away has to be written down.
    ///
    /// One C call per frame of a drag: nine primitives in, seven out, and no
    /// snapshot decoded.
    public func setGeometry(_ want: GeometryValue) throws -> GeometryValue {
        var cx: Float = 0
        var cy: Float = 0
        var w: Float = 0
        var h: Float = 0
        var angle: Float = 0
        var turns: UInt32 = 0
        var aspect: Float = 0
        try check(
            pe_session_set_geometry(
                handle,
                Float(want.centre.x), Float(want.centre.y),
                Float(want.size.width), Float(want.size.height),
                Float(want.angle),
                // 2³² is a multiple of four, so a turn count that has gone
                // below zero — a panel's anticlockwise button on an unturned
                // crop — truncates into the unsigned parameter and comes out of
                // the engine's `% 4` as the turn it meant. `UInt32(_:)` would
                // trap on it instead.
                UInt32(truncatingIfNeeded: want.turns),
                want.flipH, want.flipV, want.aspect.parameter,
                &cx, &cy, &w, &h, &angle, &turns, &aspect
            )
        )
        return GeometryValue(
            centre: CGPoint(x: Double(cx), y: Double(cy)),
            size: CGSize(width: Double(w), height: Double(h)),
            angle: Double(angle),
            turns: Int(turns),
            // The flips have no out-parameter because nothing corrects them:
            // they are stored exactly as given, so they come back from the
            // proposal.
            flipH: want.flipH, flipV: want.flipV,
            aspect: AspectLock(parameter: aspect)
        )
    }

    /// Put the crop, straighten and flips back to the whole frame.
    ///
    /// Nothing comes back because there is nothing to correct: the answer is
    /// always `GeometryValue.identity`.
    public func resetGeometry() throws {
        try check(pe_session_reset_geometry(handle))
    }

    /// Show the whole straightened source in the viewer rather than the crop.
    ///
    /// While the crop tool is open the viewer has to show what is being cut
    /// away, or there is nothing outside the rectangle to see and nothing to
    /// drag back into. A flag rather than a frame: `Geometry::enclosing` is
    /// what the frame is and the engine computes it, so this side never holds a
    /// copy of that rule.
    ///
    /// Not an edit. The document is untouched, it is not in the history, and an
    /// export renders the document either way.
    public func setCropping(_ cropping: Bool) throws {
        try check(pe_session_set_cropping(handle, cropping))
    }

    /// Where the crop sits inside the frame the viewer is showing, in that
    /// frame's own uv.
    ///
    /// With the crop tool closed the crop *is* the frame and this is the whole
    /// of it; with the tool open it is the rectangle the overlay draws.
    public func cropInFrame() throws -> CGRect {
        var u0: Float = 0
        var v0: Float = 0
        var u1: Float = 0
        var v1: Float = 0
        try check(pe_session_crop_in_frame(handle, &u0, &v0, &u1, &v1))
        return Self.rect(u0, v0, u1, v1)
    }

    /// Move the crop to a rectangle of the frame being shown, and take back the
    /// rectangle it actually landed on.
    ///
    /// **What comes back is frequently not what was passed in, and that is the
    /// point of this call** — the same contract `setGeometry` has, and the same
    /// corrections. Deliberately not `@discardableResult`: drawing the proposal
    /// instead of the answer puts a rectangle on screen the renderer does not
    /// produce, so throwing the correction away has to be written down.
    ///
    /// One C call per frame of a drag: four floats in, four out, and nothing
    /// decoded.
    public func setCropInFrame(_ rect: CGRect) throws -> CGRect {
        var u0: Float = 0
        var v0: Float = 0
        var u1: Float = 0
        var v1: Float = 0
        try check(
            pe_session_set_crop_in_frame(
                handle,
                Float(rect.minX), Float(rect.minY), Float(rect.maxX), Float(rect.maxY),
                &u0, &v0, &u1, &v1
            )
        )
        return Self.rect(u0, v0, u1, v1)
    }

    /// The engine's four edges as a rectangle. Built from the corners rather
    /// than from a width and a height, because that is what crosses.
    private static func rect(_ u0: Float, _ v0: Float, _ u1: Float, _ v1: Float) -> CGRect {
        CGRect(
            x: CGFloat(u0), y: CGFloat(v0),
            width: CGFloat(u1 - u0), height: CGFloat(v1 - v0))
    }

    // ---- comparing ----------------------------------------------------------

    /// Hold the graded picture up against the ungraded one, or stop.
    ///
    /// `wipe` is where the seam sits, as a fraction of the frame's width, and
    /// the engine keeps it **whatever the mode is** — so a caller cycling
    /// through the modes has to hand the fraction back rather than pass zero,
    /// or the next wipe starts at the left edge instead of where the user left
    /// it. `SessionStore.cycleCompare` is the one caller that does the cycling
    /// and it reads ``compare()`` first for exactly that reason.
    ///
    /// Not an edit: nothing is in the history, the document is untouched, and
    /// an export renders the document either way. Nothing needs to be open —
    /// a property of the window outlives whichever photograph is in it.
    public func setCompare(_ mode: Compare, wipe: Float) throws {
        try check(pe_session_set_compare(handle, mode.parameter, wipe))
    }

    /// Which comparison the viewer is showing, and where its seam sits.
    ///
    /// Two answers in one call because the control that wants either wants
    /// both: the button draws its state from the mode and the seam is drawn
    /// from the fraction, on the same frame. Before anything has been set the
    /// fraction is 0.5 — a first wipe begins in the middle rather than at the
    /// left edge — which is why this side reads the pair rather than starting
    /// its own mirror at zero.
    public func compare() throws -> (mode: Compare, wipe: Float) {
        var mode: UInt32 = 0
        var wipe: Float = 0
        try check(pe_session_compare(handle, &mode, &wipe))
        return (Compare(parameter: mode), wipe)
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

    // ---- scopes -------------------------------------------------------------

    /// Render the current grade at this size and bin it.
    ///
    /// The size is the scope's, not the photograph's: a waveform has one column
    /// per pixel of width, so this is how wide the panel that will draw it is.
    public func measureScopes(width: UInt32, height: UInt32) throws {
        try check(pe_session_measure(handle, width, height))
    }

    /// Which measurement the engine is holding: 0 before the first, and
    /// strictly increasing after. Comparing this before copying is what makes
    /// a 2.6 MB waveform affordable.
    public func scopeGeneration() -> UInt64 {
        pe_session_scope_generation(handle)
    }

    /// The fraction of pixels above diffuse white, which is what a clipping
    /// warning is actually about. Nil with nothing measured.
    public func overWhiteFraction() -> Double? {
        let f = pe_session_over_white_fraction(handle)
        return f < 0 ? nil : Double(f)
    }

    /// Everything measured from the last frame, or nil when there is nothing
    /// to draw.
    ///
    /// Nil is the engine saying "measure before you draw", not "this frame has
    /// no scopes": it is the answer both before the first measurement and after
    /// an edit has thrown one away.
    public func scopes() throws -> Scopes? {
        let generation = scopeGeneration()
        guard generation != 0 else { return nil }

        // The waveform asks for no peak: it is drawn against its row count, and
        // the peak is the only field the engine has to walk 655,360 counts for.
        guard let histogram = try scopeBuffer(Histogram, planes: 4),
            let logHistogram = try scopeBuffer(LogHistogram, planes: 4),
            let colour = try scopeBuffer(ColourSpread, planes: 2),
            let waveform = try scopeBuffer(Waveform, planes: 4, wantsPeak: false),
            let vectorscope = try scopeBuffer(Vectorscope, planes: 1),
            let chromaticity = try scopeBuffer(WarperChromaticity, planes: 1),
            let hueSat = try scopeBuffer(WarperHueSat, planes: 1),
            let chromaLuma = try scopeBuffer(WarperChromaLuma, planes: 1)
        else { return nil }

        return Scopes(
            histogram: histogram.levels,
            logHistogram: logHistogram.levels,
            colour: Scopes.Spread(
                hue: colour.planes[0], saturation: colour.planes[1],
                total: colour.total, peak: colour.peak),
            waveform: Scopes.WaveformCounts(
                // A waveform crosses as one row per column, each row 256 levels
                // wide, so the ABI's height is the column count.
                columns: waveform.height, levels: waveform.width, total: waveform.total,
                red: waveform.planes[0], green: waveform.planes[1],
                blue: waveform.planes[2], luma: waveform.planes[3]),
            vectorscope: vectorscope.plane,
            warper: Scopes.WarperClouds(
                chromaticity: chromaticity.plane, hueSat: hueSat.plane,
                chromaLuma: chromaLuma.plane),
            generation: generation
        )
    }

    /// One scope as the ABI hands it over: `planes` buffers of `height * width`
    /// counts, in the plane order `PeScope` documents.
    private struct ScopeBuffer {
        let width: Int
        let height: Int
        let total: UInt32
        let peak: UInt32
        let planes: [[UInt32]]

        var levels: Scopes.Levels {
            Scopes.Levels(
                red: planes[0], green: planes[1], blue: planes[2], luma: planes[3],
                total: total, peak: peak)
        }

        var plane: Scopes.Plane {
            Scopes.Plane(
                counts: planes[0], width: width, height: height, total: total, peak: peak)
        }
    }

    /// Copy one scope out of the engine.
    ///
    /// Two calls: ask the shape, then fill a buffer of exactly that size. The
    /// engine refuses a short buffer rather than truncating, so a mismatch is
    /// an error here rather than a plausible picture of a frame that does not
    /// exist. The plane count is checked against what the caller expects for
    /// the same reason — a scope with the wrong number of planes would be read
    /// as a scope of something else.
    ///
    /// Nil means nothing is measured, which is not a failure; that is the
    /// distinction `scopes()` turns into its own nil.
    private func scopeBuffer(
        _ kind: PeScope, planes expected: Int, wantsPeak: Bool = true
    ) throws -> ScopeBuffer? {
        var planes: UInt32 = 0
        var width: UInt32 = 0
        var height: UInt32 = 0
        var total: UInt32 = 0
        var peak: UInt32 = 0
        let shaped: Int32
        if wantsPeak {
            shaped = pe_session_scope_shape(
                handle, kind, &planes, &width, &height, &total, &peak)
        } else {
            shaped = pe_session_scope_shape(
                handle, kind, &planes, &width, &height, &total, nil)
        }
        guard shaped == 0 else { return nil }
        guard Int(planes) == expected else {
            throw EngineError(
                code: -1,
                message: "the engine gave \(planes) planes for a scope that has \(expected)")
        }

        let stride = Int(width) * Int(height)
        let count = expected * stride
        // A raw buffer rather than an `[UInt32]`, so the counts are copied once
        // — out of the engine and straight into the per-plane arrays — instead
        // of twice through a flat Swift array nobody keeps.
        let raw = UnsafeMutablePointer<UInt32>.allocate(capacity: max(count, 1))
        defer { raw.deallocate() }
        let written = pe_session_scope_data(handle, kind, raw, UInt32(count))
        guard written == Int32(count) else {
            throw EngineError(
                code: written,
                message: "the engine gave \(written) counts for a scope shaped "
                    + "\(planes)x\(height)x\(width)")
        }

        return ScopeBuffer(
            width: Int(width), height: Int(height), total: total, peak: peak,
            planes: (0..<expected).map {
                Array(UnsafeBufferPointer(start: raw + $0 * stride, count: stride))
            })
    }

    // ---- the set ------------------------------------------------------------
    //
    // The photographs open at once, for the filmstrip. Only one of them is
    // decoded, so what crosses here is paths, three marks and a 128-pixel
    // thumbnail — never a frame. A session showing nothing, and a session
    // showing the built-in chart, both have no set: the count is zero, the
    // readers answer nil, and asking for thumbnails of nothing does nothing.

    /// Open a set of photographs, focused on the first.
    ///
    /// The paths cross as JSON because that is what the ABI takes: a file name
    /// is not a scalar and there is no count of them known in advance, so the
    /// typed alternative is a pointer-and-length pair of a type the ABI is not
    /// allowed to name.
    ///
    /// An empty list is a refusal rather than an empty set. The engine will not
    /// hold a set of no photographs, so nothing that reads one has to cope with
    /// the case.
    public func openPaths(_ urls: [URL]) throws {
        let json = try JSONEncoder().encode(urls.map(\.path))
        try check(
            String(decoding: json, as: UTF8.self).withCString {
                pe_session_open_paths(handle, $0)
            })
    }

    /// Show a different photograph of the set, parking the current edit and
    /// taking that one's.
    ///
    /// Throws with no set open, for an index past the end, and for a photograph
    /// that will not decode. In every one of those nothing has moved: the set
    /// is still pointed where it was and the edit on screen is still the one
    /// that was there.
    ///
    /// The engine writes the outgoing edit out as part of the switch, so a
    /// caller does not flush before calling this — doing so would be a second
    /// write of the same document.
    public func focus(_ index: Int) throws {
        guard let i = UInt32(exactly: index) else {
            throw EngineError(code: -2, message: "there is no photograph \(index) in the set")
        }
        try check(pe_session_focus(handle, i))
    }

    /// How many photographs are in the set. Zero with no set open, which is the
    /// truth rather than a failure — a strip of no entries is exactly the right
    /// thing to draw for a session that has none.
    public var entryCount: Int {
        Int(max(pe_session_entry_count(handle), 0))
    }

    /// Which photograph of the set is the one on screen, or nil when there is
    /// no set. Nil rather than zero, which would name an entry that does not
    /// exist.
    public var currentEntry: Int? {
        let i = pe_session_current_entry(handle)
        return i < 0 ? nil : Int(i)
    }

    /// The path of one photograph in the set.
    ///
    /// Nil with no set open and for an index past the end. The two are not told
    /// apart because there is nothing a strip would do differently: an entry it
    /// cannot have is an entry it cannot draw.
    public func entryPath(_ index: Int) -> URL? {
        guard let i = UInt32(exactly: index) else { return nil }
        guard let raw = pe_session_entry_path(handle, i) else { return nil }
        defer { pe_string_free(raw) }
        return URL(fileURLWithPath: String(cString: raw))
    }

    /// The three marks a strip draws on one entry, in one call rather than
    /// three, because a strip asks all three of every visible entry.
    ///
    /// Nil with no set open and for an index past the end, for the same reason
    /// `entryPath` gives.
    public func entryFlags(_ index: Int) -> (edited: Bool, failed: Bool, hasThumbnail: Bool)? {
        guard let i = UInt32(exactly: index) else { return nil }
        var edited = false
        var failed = false
        var hasThumbnail = false
        guard pe_session_entry_flags(handle, i, &edited, &failed, &hasThumbnail) == 0 else {
            return nil
        }
        return (edited, failed, hasThumbnail)
    }

    /// Ask for the thumbnails of `range` that have not been asked for yet.
    ///
    /// The range the strip is actually showing, not the whole set: opening a
    /// folder of a thousand should not queue a thousand decodes before the
    /// first one anybody can see. The decode happens on a worker thread and the
    /// pixels arrive later, through `collectThumbnails`. Asking twice costs
    /// nothing — the second ask is dropped.
    ///
    /// Not throwing, and a no-op with no set open: a session with no
    /// photographs answers "give me your thumbnails" by having none, not by
    /// failing.
    public func requestThumbnails(_ range: Range<Int>) {
        let from = max(range.lowerBound, 0)
        let to = max(range.upperBound, from)
        guard let a = UInt32(exactly: from), let b = UInt32(exactly: to) else { return }
        _ = pe_session_request_thumbnails(handle, a, b)
    }

    /// Take delivery of whatever the thumbnail worker has finished. True if
    /// anything arrived.
    ///
    /// **This answer is the whole mechanism**, the way the scope generation is
    /// for the counts: it is what tells a holder of copies that any of them
    /// have moved. A thumbnail is 64 KB and a set can be two hundred of them,
    /// so copying on a schedule instead of on this answer is thirteen megabytes
    /// per frame.
    @discardableResult
    public func collectThumbnails() -> Bool {
        pe_session_collect_thumbnails(handle) == 1
    }

    /// Copy one entry's thumbnail out of the engine.
    ///
    /// Two calls, the same shape-then-fill `scopeBuffer` uses: ask the size,
    /// then fill a buffer of exactly that size. The engine refuses a short
    /// buffer rather than truncating, so a mismatch is an error here rather
    /// than a plausible-looking photograph with its last rows missing.
    ///
    /// Nil means there is nothing to copy — no set, an index past the end, or a
    /// thumbnail the worker has not delivered yet — which is not a failure.
    public func thumbnail(_ index: Int) throws -> Thumbnail? {
        guard let i = UInt32(exactly: index) else { return nil }
        var width: UInt32 = 0
        var height: UInt32 = 0
        guard pe_session_thumbnail_shape(handle, i, &width, &height) == 0 else { return nil }

        let count = Int(width) * Int(height) * 4
        guard count > 0 else {
            throw EngineError(
                code: -1, message: "the engine gave a thumbnail shaped \(width)x\(height)")
        }
        var rgba = [UInt8](repeating: 0, count: count)
        let written = rgba.withUnsafeMutableBufferPointer {
            pe_session_thumbnail_data(handle, i, $0.baseAddress, UInt32(count))
        }
        guard written == Int32(count) else {
            throw EngineError(
                code: written,
                message: "the engine gave \(written) bytes for a thumbnail shaped "
                    + "\(width)x\(height)")
        }
        return Thumbnail(width: Int(width), height: Int(height), rgba: rgba)
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

    /// Screen pixels per image pixel, or nil with nothing to measure.
    ///
    /// Not the view's zoom, which is a fraction of the frame and reads 1 for
    /// any fitted view however big the window is.
    public var viewScale: CGFloat? {
        let s = pe_session_view_scale(handle)
        return s > 0 ? CGFloat(s) : nil
    }

    // ---- the grade in hand -------------------------------------------------

    /// Copy this photograph's grade — the whole stack, pinned rows included.
    public func copyGrade() throws {
        try check(pe_session_copy_grade(handle))
    }

    /// Whether a grade has been copied, which is what the Paste items are
    /// greyed by. Never throws: "no session" and "nothing copied" both mean
    /// there is nothing to paste, and a menu has one way to say that.
    public var hasGrade: Bool {
        pe_session_has_grade(handle) != 0
    }

    /// Put the copied grade on this photograph, as one undo step.
    public func pasteGrade() throws {
        try check(pe_session_paste_grade(handle))
    }

    /// Put it on every *other* photograph in the set, returning how many took
    /// it. Zero is a real answer for a set of one.
    @discardableResult
    public func pasteGradeToAll() throws -> Int {
        let n = pe_session_paste_grade_to_all(handle)
        guard n >= 0 else {
            throw EngineError(code: n, message: lastError ?? "no reason given")
        }
        return Int(n)
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

    // ---- what is remembered between runs -------------------------------------
    //
    // The handful of things that belong to the person rather than to any one
    // picture: the effects they have starred and the set that was open. They
    // live in the engine and not in `@AppStorage` because a star means the same
    // thing in both shells, and so does the set you left open — an answer that
    // depends on which application you happened to open is an answer given
    // twice. What stays in `@AppStorage` is per-window interface state: which
    // tool is showing, whether the scopes are open.

    /// Whether an effect is starred.
    ///
    /// Not throwing: a key the engine has never heard of is not starred, which
    /// is an answer rather than a failure, and the one remaining `-1` is a null
    /// handle that a session cannot have.
    public func isFavourite(_ key: String) -> Bool {
        key.withCString { pe_session_is_favourite(handle, $0) == 1 }
    }

    /// Star or unstar an effect, and write the change out.
    ///
    /// Written immediately rather than on the way out, because a window can be
    /// closed by the operating system, by a crash, or by somebody who does not
    /// think of starring as something that needs committing.
    public func toggleFavourite(_ key: String) throws {
        try check(key.withCString { pe_session_toggle_favourite(handle, $0) })
    }

    /// Every starred effect, in the order they were starred.
    ///
    /// A list rather than a question per effect: the browser asks about all
    /// thirty, and thirty calls to answer one question is thirty crossings of
    /// the boundary for a list the engine already holds.
    public func favourites() throws -> [String] {
        guard let raw = pe_session_favourites_json(handle) else {
            throw EngineError(code: -1, message: "the engine produced no favourites")
        }
        defer { pe_string_free(raw) }
        return try JSONDecoder().decode([String].self, from: Data(String(cString: raw).utf8))
    }

    /// The set that was open when this last ran, and which one was showing.
    ///
    /// **Only the photographs that are still there.** One that has been moved,
    /// renamed, or left on a volume that is not mounted is dropped, and which
    /// one was showing is looked up again by name in what survived — so losing
    /// one from the front of the set does not slide the answer onto its
    /// neighbour.
    ///
    /// An empty set with an index of nought is a first run, and also a set
    /// whose files have all gone. The index is not a position in it.
    ///
    /// **What this does not promise is that the photographs will decode.** They
    /// exist; that is all the engine can say without opening every one of them
    /// at launch. See ``SessionStore/openRemembered()``, which is where this
    /// shell decides what to do about the one that will not.
    public func rememberedSession() throws -> (paths: [URL], index: Int) {
        guard let raw = pe_session_remembered_session_json(handle) else {
            throw EngineError(code: -1, message: "the engine remembered no session")
        }
        defer { pe_string_free(raw) }
        let remembered = try JSONDecoder().decode(
            Remembered.self, from: Data(String(cString: raw).utf8))
        return (remembered.paths.map { URL(fileURLWithPath: $0) }, remembered.index)
    }

    /// The shape `pe_session_remembered_session_json` writes.
    private struct Remembered: Decodable {
        let paths: [String]
        let index: Int
    }

    // ---- a batch ------------------------------------------------------------
    //
    // Every photograph in the set, each with its own edit, into one folder
    // chosen rather than beside each original. The run belongs to the engine;
    // what crosses here is a directory, a step, three counts and a cancel.
    // Never a frame, and never a list.
    //
    // **The stepping is this side's, and that is deliberate.** Sixty
    // photographs is sixty full-resolution renders, and a loop inside the
    // engine would freeze the window for a minute with no way to tell whether
    // it was working or hung, and no way to stop it. One step per frame keeps
    // the interface alive, gives somewhere to show progress, and makes
    // cancelling a matter of not asking for the next one.

    /// Begin exporting every photograph in the set into `directory`, in
    /// whichever format ``setExport(format:quality:)`` was last given.
    ///
    /// The format is taken now rather than per photograph, so changing it
    /// halfway cannot leave a folder half JPEG and half PNG.
    ///
    /// Throws with no set open. A session showing nothing, and a session
    /// showing the built-in chart, have no photographs to run over, and a
    /// successful run of nought files is not the honest answer to either.
    /// Starting a second run replaces the first, counts and all.
    public func startBatch(into directory: URL) throws {
        try check(directory.path.withCString {
            pe_session_start_batch(handle, $0)
        })
    }

    /// Export one photograph, and say whether there is more to do.
    ///
    /// **True while there is more**, false when there is not, and a throw when
    /// the run was refused. The ABI puts "more to do" on `1` rather than on the
    /// usual 0-is-success precisely so that the loop condition is `> 0` and
    /// never `!= 0`: reading `0` as a failure abandons a run on its last step
    /// and leaves the rest of the folder unwritten, and reading a negative as
    /// "finished" reports `n exported` for a run that never started.
    ///
    /// A throw is the engine having no device to render with, which ends the
    /// whole run rather than costing it one photograph. A photograph that
    /// merely could not be written — a collision with somebody's original, a
    /// file that will not decode — is *not* a throw: it is counted in
    /// ``BatchCounts/failed`` and stepped past, because one collision should
    /// not abandon the other sixty-five.
    ///
    /// False with no run in progress at all, since there is equally nothing
    /// more to do. ``batchProgress()`` is what tells a run that finished from
    /// one that was never started.
    ///
    /// Call it once a frame from wherever the render loop already ticks, not in
    /// a loop and never inside a view update: a full-resolution render there is
    /// a frozen window with extra steps.
    public func stepBatch() throws -> Bool {
        let answer = pe_session_step_batch(handle)
        guard answer >= 0 else {
            throw EngineError(code: answer, message: lastError ?? "no reason given")
        }
        return answer > 0
    }

    /// How far the run has got, or nil when no run has been started.
    ///
    /// Three counts in one call, because a progress bar wants all three on
    /// every frame it draws. Nil rather than a throw, and the ABI deliberately
    /// leaves its last error alone for this one: "no batch is running" is the
    /// ordinary state of a session, this is asked once a frame to decide
    /// whether to draw a bar at all, and a message per frame would bury
    /// whatever real failure was sitting there.
    ///
    /// **A finished run is still a run.** Its counts stay readable until it is
    /// cancelled or another begins, which is what makes the summary — `n
    /// exported`, or `n exported, m failed` — readable *after* the step that
    /// answered false. A run that silently stopped is indistinguishable from
    /// one that crashed.
    public func batchProgress() -> BatchCounts? {
        var done: UInt32 = 0
        var failed: UInt32 = 0
        var total: UInt32 = 0
        guard pe_session_batch_progress(handle, &done, &failed, &total) == 0 else {
            return nil
        }
        return BatchCounts(done: Int(done), failed: Int(failed), total: Int(total))
    }

    /// Stop the run, keeping whatever has already been written.
    ///
    /// Nothing is taken back. Half a folder of exports is the state somebody
    /// asked for when they pressed stop; deleting the files they had already
    /// waited for would be the surprising answer. Also how the counts of a
    /// *finished* run are put away once the summary has been read.
    ///
    /// Cancelling when nothing is running does nothing, so a window closing
    /// does not have to know whether a run is on.
    public func cancelBatch() {
        _ = pe_session_cancel_batch(handle)
    }
}

/// How far a batch export has got: written, missed, and how many there are.
///
/// A value type and `Equatable`, so the store can write it only when it has
/// actually moved rather than telling the bar drawing it to run its body again
/// sixty times a second for three numbers it already has.
public struct BatchCounts: Equatable, Sendable {
    /// Photographs written.
    public let done: Int
    /// Photographs that could not be written — a collision with somebody's
    /// original, a file that will not decode. Counted and stepped past, never a
    /// stop: one collision should not abandon the other sixty-five.
    public let failed: Int
    /// How many the run was started over. Snapshotted when it started, so a
    /// photograph taken out of the set part way through is still on disc and
    /// still worth exporting.
    public let total: Int

    public init(done: Int, failed: Int, total: Int) {
        self.done = done
        self.failed = failed
        self.total = total
    }

    /// Whether every photograph has been accounted for.
    ///
    /// The two counts do not have to add up to the third until the run is over,
    /// which is the point of being given all three.
    public var finished: Bool { done + failed >= total }

    /// What a bar fills, 0 to 1. A run over nothing reads as finished rather
    /// than as empty — though the engine refuses to start one.
    public var fraction: Double {
        guard total > 0 else { return 1 }
        return min(Double(done + failed) / Double(total), 1)
    }
}

extension AspectLock {
    /// The lock as the ABI's single `aspect` float.
    ///
    /// `AspectLock` has three arms and this is one number, because the
    /// alternative on a drag path is an enum across the ABI plus a second
    /// parameter to carry its payload. A positive number is a width-to-height
    /// ratio, `PE_ASPECT_ORIGINAL` is the source's own proportions, and zero —
    /// like anything else at or below it — is free.
    ///
    /// A ratio loses its spelling on the way across: 16:9 goes out as 1.777…
    /// and comes back as `.ratio(w: 1.777…, h: 1)`. Same lock, and all the crop
    /// arithmetic ever wanted; the snapshot is where a panel reads the two
    /// numbers it needs to *print* one.
    ///
    /// The divisor's guard is `aspect_value`'s in `pe-ffi`, so a malformed lock
    /// crosses as a finite number rather than an infinity — which would be read
    /// back as free, quietly dropping the lock.
    var parameter: Float {
        switch self {
        case .free: return 0
        case .original: return Float(PE_ASPECT_ORIGINAL)
        case let .ratio(w, h): return Float(w / max(h, 1e-6))
        }
    }

    /// Read the ABI's `aspect` back — the mirror of `aspect_lock` in `pe-ffi`,
    /// including what it does with infinity and NaN: neither is a ratio a crop
    /// can hold, so both are free rather than a lock nothing can satisfy.
    ///
    /// `PE_ASPECT_ORIGINAL` crosses the bridging header as a `Double`, so it is
    /// narrowed here. It is exactly representable, which is what makes the
    /// comparison an equality and not a tolerance.
    init(parameter: Float) {
        if parameter == Float(PE_ASPECT_ORIGINAL) {
            self = .original
        } else if parameter > 0, parameter.isFinite {
            self = .ratio(w: Double(parameter), h: 1)
        } else {
            self = .free
        }
    }
}

extension Compare {
    /// The mode as the ABI's `mode` integer — `PE_COMPARE_OFF`,
    /// `PE_COMPARE_WIPE`, `PE_COMPARE_SIDE`.
    ///
    /// The numbering is part of the ABI and the constants are generated into
    /// the header, so this switch is the only place on this side that knows
    /// which integer is which. An exhaustive switch rather than a raw value on
    /// the enum, because `Compare` is a shell idea — the cycle and the label
    /// are not the engine's business — and pinning its cases to the ABI's
    /// integers would make adding a fourth way of comparing a change to both.
    ///
    /// The constants cross the bridging header as `Int32`, because that is what
    /// a bare `#define 0` is in C; the ABI's parameter is unsigned, so they are
    /// widened here rather than at each call.
    var parameter: UInt32 {
        switch self {
        case .off: UInt32(PE_COMPARE_OFF)
        case .wipe: UInt32(PE_COMPARE_WIPE)
        case .side: UInt32(PE_COMPARE_SIDE)
        }
    }

    /// Read the ABI's `mode` back.
    ///
    /// `pe_session_compare` answers with one of the three it was given —
    /// `pe_session_set_compare` refuses anything else rather than quietly
    /// storing it — so the default arm is unreachable through this ABI. It is
    /// off rather than a crash because the alternative is a shell that will not
    /// open against a later engine that grew a fourth mode.
    init(parameter: UInt32) {
        switch parameter {
        case UInt32(PE_COMPARE_WIPE): self = .wipe
        case UInt32(PE_COMPARE_SIDE): self = .side
        default: self = .off
        }
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
