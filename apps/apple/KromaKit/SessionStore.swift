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
        if session.needsRender {
            run { try session.render() }
        }
        // After the frame, not before: the scopes describe what was just drawn.
        // Costs nothing at all unless a panel is open and the counts are stale.
        measureScopesIfNeeded()
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

    // ---- crop, straighten, flips ------------------------------------------

    /// The geometry to draw the crop overlay and the crop panel from.
    ///
    /// Mid-drag the snapshot is deliberately stale, so this is the engine's
    /// corrected answer to the last proposal rather than the snapshot's copy of
    /// it. Between drags the two are the same value, which is why this reads
    /// through to the snapshot rather than keeping a copy of its own.
    public var geometry: GeometryValue { corrected ?? snapshot.geometry }

    /// What the engine last accepted, held only while the snapshot is behind.
    private var corrected: GeometryValue?

    /// Propose a crop, straighten and flips.
    ///
    /// The overlay then draws `geometry`, which is what the engine **stored** —
    /// not what was proposed. Drawing the proposal would put a rectangle on
    /// screen that the renderer does not produce, and it would jump to the real
    /// one the moment the drag ended.
    ///
    /// Like `setFloat`, this does not refresh the snapshot mid-drag: the
    /// corrected value above is what the overlay reads until the gesture ends.
    public func setGeometry(_ want: GeometryValue) {
        run { corrected = try session.setGeometry(want) }
        if !dragging { refresh() }
    }

    /// Put the crop, straighten and flips back to the whole frame.
    public func resetGeometry() {
        run { try session.resetGeometry() }
        refresh()
    }

    /// Whether the viewer is showing the whole straightened source rather than
    /// the crop. See ``SessionStore/setCropping(_:)``.
    public private(set) var cropping = false

    /// Open or close the crop tool's view of the photograph.
    ///
    /// The engine renders the enclosing frame while this is on, so the overlay
    /// has something outside the rectangle to draw over and the user has
    /// something to drag back into. Nothing about the document changes, so
    /// there is no edit and nothing to undo.
    public func setCropping(_ cropping: Bool) {
        run { try session.setCropping(cropping) }
        self.cropping = cropping
        // The frame changed, so where the crop sits in it changed with it.
        refreshCropRect()
    }

    /// Where the crop sits inside the frame the viewer is showing, in that
    /// frame's own uv — the engine's answer, never this side's.
    ///
    /// Mid-drag this is what the engine wrote back to the last proposal, which
    /// is what the overlay draws; between drags it is re-read after every edit.
    /// The whole frame is the answer with nothing open and with the tool
    /// closed, which is also what makes it a harmless default.
    public private(set) var cropRect = CGRect(x: 0, y: 0, width: 1, height: 1)

    /// Propose a rectangle of the frame being shown, and keep the corrected one.
    ///
    /// The overlay then draws ``SessionStore/cropRect``, which is what the
    /// engine **stored** — not what was proposed. Like `setFloat`, this does
    /// not refresh the snapshot mid-drag; the rectangle above is what the
    /// overlay reads until the gesture ends.
    public func setCropRect(_ rect: CGRect) {
        run { cropRect = try session.setCropInFrame(rect) }
        if !dragging { refresh() }
    }

    /// Re-read where the crop is. One C call and four floats — cheap enough to
    /// do after every edit, which is what keeps it from going stale behind an
    /// undo or a panel.
    ///
    /// A refusal here is "nothing is open", which is not something to put in
    /// the status bar: there is no photograph to draw a rectangle on, and the
    /// whole frame is the honest answer for one that has no crop.
    private func refreshCropRect() {
        let rect = (try? session.cropInFrame()) ?? CGRect(x: 0, y: 0, width: 1, height: 1)
        // Only written when it is actually changing: assigning an equal value
        // would tell every observing view to run its body again for nothing.
        if rect != cropRect { cropRect = rect }
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

    /// What a visible scopes panel wants measured, or nil when no panel is on
    /// screen.
    ///
    /// The visibility lives here rather than in the panel because it is the
    /// store that decides to spend the measurement, and a measurement is a full
    /// extra render plus a 1.2 MB readback. Paid behind a closed panel that is
    /// the kind of cost nobody attributes correctly later, so the panel says
    /// when it is looking and the store measures only then.
    @ObservationIgnored public private(set) var scopeRequest: ScopeSize?

    /// What the held counts were measured at. A panel that has been made wider
    /// wants more columns, and a waveform stretched from three hundred to six
    /// is a picture of the interpolation.
    @ObservationIgnored private var scopeMeasured: ScopeSize?

    /// Say whether a scopes panel is on screen, and at what size.
    public func requestScopes(_ size: ScopeSize?) {
        scopeRequest = size
    }

    /// Measure if a panel is looking and what it would draw is stale.
    ///
    /// Called from the frame tick. The answer says whether the measurement was
    /// actually taken, which is the one thing worth knowing about a call that
    /// renders the photograph a second time.
    @discardableResult
    public func measureScopesIfNeeded() -> Bool {
        guard let request = scopeRequest, snapshot.isOpen else { return false }
        // Every edit throws the engine's measurement away, and mid-drag this is
        // the only place the store hears about it: `setFloat` and its kin
        // deliberately skip `refresh` while a gesture is in flight. Without this
        // line the scopes freeze under the hand that is grading, which is
        // exactly when they are being watched.
        syncScopes()
        guard scopes == nil || scopeMeasured != request else { return false }
        scopeMeasured = request
        measureScopes(width: request.width, height: request.height)
        return true
    }

    /// Bring the copy into step with the engine, doing nothing when it already
    /// is.
    ///
    /// One integer against 2.6 MB.
    ///
    /// The generation answers both questions on its own: zero means there is
    /// nothing to read — either nothing has been measured yet, or an edit threw
    /// the last measurement away — and any other value is the identity of the
    /// counts being held. Counts kept across an edit would draw a scope of a
    /// picture that is not on screen.
    private func syncScopes() {
        let generation = session.scopeGeneration()
        guard generation != scopeGeneration else { return }
        scopeGeneration = generation
        guard generation != 0 else {
            // Only written when it is actually changing: assigning nil over nil
            // would tell every observing view to run its body again for nothing.
            if scopes != nil { scopes = nil }
            return
        }
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
        // The snapshot is the truth again, so the corrected geometry held
        // through the drag is not needed. Only written when it is actually
        // changing: assigning nil over nil would tell every observing view to
        // run its body again for nothing.
        if corrected != nil { corrected = nil }
        // Before the version check, not after it. Where the crop sits is
        // measured against the frame the *viewer* is showing, and that frame is
        // not in the snapshot — the version tracks the document alone, so a
        // rectangle refreshed only when the version moved would be one the
        // engine had already stopped agreeing with.
        refreshCropRect()
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
