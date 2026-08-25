import CoreGraphics
import Foundation
import Observation
import QuartzCore

/// What the interface reads, and what it calls to change anything.
///
/// The flow is one-way in both directions. Swift mutates through typed calls;
/// the engine's state comes back as an immutable `Snapshot`. Nothing here
/// authors document state, which is what stops this becoming a second
/// implementation of the document that has to be kept in step with the first.
///
/// Main-actor bound because the engine is single-threaded and the display link
/// calls into it from the main thread. That is the same arrangement the
/// Windows shell has, where the frame loop owns everything.
@MainActor
@Observable
public final class SessionStore {
    public private(set) var snapshot: Snapshot = .empty
    public let registry: Registry
    /// The last thing that went wrong, for the status bar. Cleared by the next
    /// call that succeeds.
    public private(set) var problem: String?

    @ObservationIgnored private let session: Session
    /// Set while a drag is in flight. The snapshot is not refreshed until it
    /// ends: the control holds the in-flight value locally, so the cost per
    /// frame is one call and one render of one cached stage.
    @ObservationIgnored private var dragging = false

    public init?() {
        guard let session = Session() else { return nil }
        guard let registry = try? Engine.registry() else { return nil }
        self.session = session
        self.registry = registry
    }

    // ---- what the viewer needs ------------------------------------------

    public var needsRender: Bool { session.needsRender }
    public var lastPasses: Int { session.lastPasses }

    public func attach(layer: CALayer, width: UInt32, height: UInt32) {
        run { try session.attach(layer: layer, width: width, height: height) }
    }

    public func resize(width: UInt32, height: UInt32) {
        run { try session.resize(width: width, height: height) }
    }

    public func detachLayer() { session.detachLayer() }

    // ---- where the viewer is looking -------------------------------------

    /// Where the viewer is looking. Held here rather than in the engine
    /// because it belongs to the window, not to the photograph.
    public private(set) var view = ViewState()

    public func zoom(by factor: CGFloat, at viewPoint: CGPoint) {
        view.zoom(by: factor, at: viewPoint)
        pushView()
    }

    public func pan(by delta: CGSize) {
        view.pan(by: delta)
        pushView()
    }

    public func fitView() {
        view.fit()
        pushView()
    }

    private func pushView() {
        let r = view.region
        run {
            try session.setView(
                x: Float(r.origin.x), y: Float(r.origin.y), size: Float(r.width))
        }
    }

    /// Draw, if anything has changed. Called from the display link.
    public func renderIfNeeded() {
        // The tick drives the autosave debounce, so it can fail with something
        // the person needs to know — that their work in progress is not being
        // written. It goes to the status bar like any other refusal, and does
        // not stop the frame being drawn.
        run { try session.tick() }
        guard session.needsRender else { return }
        run { try session.render() }
    }

    // ---- opening ---------------------------------------------------------

    public func setSupportDirectory(_ url: URL) {
        run { try session.setSupportDirectory(url) }
    }

    public func openTestChart(width: UInt32 = 1024, height: UInt32 = 768) {
        run { try session.openTestChart(width: width, height: height) }
        refresh()
    }

    public func open(_ url: URL) {
        flush()
        run { try session.open(url) }
        refresh()
    }

    // ---- editing ---------------------------------------------------------

    @discardableResult
    public func addEffect(_ key: String) -> UInt64? {
        var id: UInt64?
        run { id = try session.addEffect(key) }
        refresh()
        return id
    }

    public func removeRow(_ row: UInt64) {
        run { try session.removeRow(row) }
        refresh()
    }

    public func moveRow(_ row: UInt64, to index: UInt32) {
        run { try session.moveRow(row, to: index) }
        refresh()
    }

    public func setRowOpacity(_ row: UInt64, _ value: Float) {
        run { try session.setRowOpacity(row, value) }
        if !dragging { refresh() }
    }

    /// Whether this row may be taken out of the stack.
    ///
    /// The pinned rows are the colour page's fixed panels; a document without
    /// them is one a fresh document could not be, and an inspector with a hole
    /// in it. The engine would allow it, which is why the answer lives here
    /// rather than being assumed.
    public func canRemove(_ row: Snapshot.Row) -> Bool {
        !row.pinned
    }

    /// Bracket a drag. Between these two calls the snapshot is left alone.
    public func beginInteraction(_ label: String) {
        dragging = true
        session.beginInteraction(label)
    }

    public func endInteraction() {
        session.endInteraction()
        dragging = false
        refresh()
    }

    /// The hot path: one call, no snapshot, no allocation beyond the key.
    public func setFloat(row: UInt64, key: String, value: Float) {
        run { try session.setFloat(row: row, key: key, value: value) }
        if !dragging { refresh() }
    }

    /// The wheel's hot path. Like `setFloat`, it does not refresh the snapshot
    /// mid-drag — the control holds the in-flight value and draws from that.
    public func setWheel(
        row: UInt64, key: String, master: Float, _ r: Float, _ g: Float, _ b: Float
    ) {
        run { try session.setWheel(row: row, key: key, master: master, r, g, b) }
        if !dragging { refresh() }
    }

    /// A curve's hot path. Like `setFloat`, it does not refresh the snapshot
    /// mid-drag — the editor holds the in-flight curve and draws from that.
    public func setCurve(row: UInt64, key: String, points: [CGPoint]) {
        run { try session.setCurve(row: row, key: key, points: points) }
        if !dragging { refresh() }
    }

    /// A lattice's hot path. Like `setFloat`, it does not refresh the snapshot
    /// mid-drag — the editor holds the in-flight lattice and draws from that.
    public func setWarpVertex(
        row: UInt64, key: String, col: Int, vertexRow: Int, offset: CGPoint
    ) {
        run {
            try session.setWarpVertex(
                row: row, key: key, col: col, vertexRow: vertexRow, offset: offset
            )
        }
        if !dragging { refresh() }
    }

    public func clearWarp(row: UInt64, key: String) {
        run { try session.clearWarp(row: row, key: key) }
        refresh()
    }

    /// Place a pin, answering with its index so the panel can select what it
    /// just made.
    ///
    /// Nil means no pin was added — `run` keeps the refusal for the status bar
    /// rather than throwing, so the index simply never arrives, and selecting
    /// on the strength of an index that is not there would select a pin that
    /// does not exist.
    @discardableResult
    public func addPin(row: UInt64, key: String, at: CGPoint) -> Int? {
        var index: Int?
        run { index = try session.addPin(row: row, key: key, at: at) }
        refresh()
        return index
    }

    /// A pin's hot path. Like `setFloat`, it does not refresh the snapshot
    /// mid-drag — the editor holds the in-flight pin and draws from that.
    public func movePin(row: UInt64, key: String, index: Int, to: CGPoint) {
        run { try session.movePin(row: row, key: key, index: index, to: to) }
        if !dragging { refresh() }
    }

    /// The other hot path: the five shape controls, carried together so a
    /// slider drag is one call and one undo step.
    public func setPinShape(
        row: UInt64, key: String, index: Int,
        chromaRange: Double, tonalLow: Double, tonalHigh: Double,
        tonalPivot: Double, exposure: Double
    ) {
        run {
            try session.setPinShape(
                row: row, key: key, index: index, chromaRange: chromaRange,
                tonalLow: tonalLow, tonalHigh: tonalHigh, tonalPivot: tonalPivot,
                exposure: exposure
            )
        }
        if !dragging { refresh() }
    }

    public func removePin(row: UInt64, key: String, index: Int) {
        run { try session.removePin(row: row, key: key, index: index) }
        refresh()
    }

    public func setBool(row: UInt64, key: String, value: Bool) {
        run { try session.setBool(row: row, key: key, value: value) }
        refresh()
    }

    public func setChoice(row: UInt64, key: String, value: String) {
        run { try session.setChoice(row: row, key: key, value: value) }
        refresh()
    }

    public func setRGB(row: UInt64, key: String, _ r: Float, _ g: Float, _ b: Float) {
        run { try session.setRGB(row: row, key: key, r, g, b) }
        refresh()
    }

    public func setRowEnabled(_ row: UInt64, _ on: Bool) {
        run { try session.setRowEnabled(row, on) }
        refresh()
    }

    // ---- the scopes ------------------------------------------------------

    /// The last measurement, or nil when there is nothing to draw.
    ///
    /// A stored property, not a call. A waveform at 640 columns is 2.6 MB and a
    /// scope panel's body runs whenever anything it observes moves, so a
    /// `scopes()` that copied on every read would stutter the window exactly
    /// while a slider is being dragged — which is when a colourist is watching
    /// the scopes. The copy happens once per measurement, in `syncScopes`, and
    /// every body after that reads the same arrays.
    public private(set) var scopes: Scopes?

    /// Which measurement `scopes` holds. Compared before copying; that
    /// comparison is the whole mechanism.
    @ObservationIgnored private var scopeGeneration: UInt64 = 0

    /// Measure the graded frame at this size.
    ///
    /// The size is the scope's, not the photograph's: a waveform has one column
    /// per pixel of width, so this is how wide the panel that will draw it is.
    public func measureScopes(width: UInt32, height: UInt32) {
        run { try session.measureScopes(width: width, height: height) }
        syncScopes()
    }

    /// Bring the copy into step with the engine, doing nothing when it already
    /// is.
    ///
    /// Two cheap questions before any copy. `hasScopes` first, because an edit
    /// throws the measurement away without advancing the generation — counts
    /// kept across an edit would draw a scope of a picture that is not on
    /// screen. Then the generation, which is one integer against 2.6 MB.
    private func syncScopes() {
        guard session.hasScopes else {
            scopeGeneration = 0
            // Only written when it is actually changing: assigning nil over nil
            // would tell every observing view to run its body again for nothing.
            if scopes != nil { scopes = nil }
            return
        }
        let generation = session.scopeGeneration()
        guard generation != scopeGeneration else { return }
        scopeGeneration = generation
        run { scopes = try session.scopes() }
    }

    // ---- history ---------------------------------------------------------

    public var canUndo: Bool { snapshot.canUndo }
    public var canRedo: Bool { snapshot.canRedo }

    public func undo() {
        run { _ = try session.undo() }
        refresh()
    }

    public func redo() {
        run { _ = try session.redo() }
        refresh()
    }

    // ---- persistence and export ------------------------------------------

    public func revert() {
        run { try session.revert() }
        refresh()
    }

    @discardableResult
    public func export() -> URL? {
        var out: URL?
        run { out = try session.export() }
        refresh()
        return out
    }

    public func setExport(format: String, quality: UInt8) {
        run { try session.setExport(format: format, quality: quality) }
        refresh()
    }

    /// Write the work in progress now. Called when leaving a photograph and
    /// when the window closes.
    public func flush() {
        run { try session.flushAutosave() }
    }

    // ---- the mechanism ----------------------------------------------------

    /// Pull the engine's state across, but only when it has actually moved.
    ///
    /// The version is one integer; the snapshot is a document. Comparing the
    /// former before decoding the latter is what makes mirroring cheap enough
    /// to do after every structural edit.
    private func refresh() {
        // Any edit throws the measurement away, and this is where the store
        // hears about an edit. Two integers and a pointer test, not a copy.
        syncScopes()
        guard session.snapshotVersion != snapshot.version else { return }
        do {
            snapshot = try session.snapshot()
        } catch {
            problem = String(describing: error)
        }
    }

    /// Run an engine call, keeping whatever it said if it refused.
    ///
    /// A refusal is not an exception here — "that export would land on one of
    /// your photographs" is the application working. It belongs in the status
    /// bar, not in a crash.
    private func run(_ body: () throws -> Void) {
        do {
            try body()
            problem = nil
        } catch {
            problem = String(describing: error)
        }
    }
}
