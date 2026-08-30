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

    /// The last thing that went *right* and is worth saying out loud.
    ///
    /// A separate channel from ``problem`` because the status bar draws that
    /// one in the error colour, and "grade pasted to 3 photos" is not an error.
    /// `status.done` in `main.rs` is the same idea.
    ///
    /// Only for work whose result is otherwise invisible. Pasting to the *other*
    /// photographs in a set changes nothing on screen, so without this the
    /// command looks like it did nothing at all. An ordinary edit needs no
    /// notice: the picture is the notice.
    public private(set) var notice: String?

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
        // Read rather than assumed: a fresh session's seam is already in the
        // middle, and a mirror that started at zero would put the first wipe
        // hard against the left edge.
        refreshCompare()
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

    /// Whether the whole picture is already on screen, which is what the Fit
    /// button is greyed by.
    public var isFit: Bool { view.zoom == 1 }

    /// One line naming the GPU in use, or nil until a device exists.
    ///
    /// Read straight off the engine rather than cached, for the same reason
    /// ``viewScale`` is: it is nil until the first frame and nothing tells the
    /// store when that happens.
    public var gpuName: String? { session.gpuName }

    /// Screen pixels per image pixel, or nil with nothing to measure.
    ///
    /// Read straight off the engine every time rather than cached: it changes
    /// when the window is resized, and nothing tells the store about that.
    public var viewScale: CGFloat? { session.viewScale }

    /// One image pixel to one screen pixel.
    ///
    /// `viewScale` is what the current zoom is worth, so the factor that takes
    /// it to 1 is what the zoom has to be multiplied by. `ViewState` clamps the
    /// result, which is what stops a photograph smaller than the window from
    /// asking to be zoomed out past fit — there is no such view.
    ///
    /// About the middle of the viewport, so what was in the centre stays there.
    /// Zooming about a corner would send the subject off screen.
    public func zoomToActualPixels() {
        guard let scale = viewScale, scale > 0 else { return }
        view.zoom(by: 1 / scale, at: CGPoint(x: 0.5, y: 0.5))
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
        // At the top of the frame, and before anything else: thumbnails arrive
        // on a worker thread, so this is the only place the store hears about
        // them. One C call when nothing has turned up, which is almost every
        // frame.
        collectThumbnails()
        // One photograph of a batch, here and in no `body`: a full-resolution
        // render inside a view update is a frozen window with extra steps.
        // Before the frame rather than after it, so what the bar draws this
        // time round is the count the step just moved.
        stepBatch()
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
        // Being told where the support directory is *is* how the engine comes
        // to read a settings file, so this is the first moment in a launch at
        // which there are any stars to know about.
        refreshFavourites()
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

    // ---- the set ---------------------------------------------------------

    /// The photographs open, and which of them is on screen.
    ///
    /// Empty for a session showing nothing and for one showing the built-in
    /// chart, which is not a file and therefore not a set of one. One
    /// photograph opened on its own *is* a set of one — `open` goes through the
    /// same door — so a strip has an entry to draw for it.
    ///
    /// A stored property, so a strip's body reads a value rather than walking
    /// the ABI.
    public private(set) var library: Library = .empty

    /// The thumbnails, as pictures, keyed by the photograph's path.
    ///
    /// By path rather than by index for the reason `Library::index_of` is a
    /// search rather than a stored number: the set shifts under every removal,
    /// and a picture that ends up on the wrong entry is a filmstrip quietly
    /// showing the wrong photograph.
    ///
    /// Not observed. What a view watches is `library`, which is written when
    /// the marks move — including `hasThumbnail`. Observing 13 MB of pictures
    /// as well would be a second notification for the same event.
    @ObservationIgnored private var thumbnails: [URL: CGImage] = [:]

    /// Open a set of photographs, focused on the first.
    ///
    /// An empty list is refused by the engine rather than opening a set of
    /// none, and the refusal arrives in `problem` like any other.
    /// Open every photograph in a folder, and say how many.
    ///
    /// The count is worth saying: a folder that opened is otherwise
    /// indistinguishable from one that did not, and this is the command that
    /// turns a single photograph into a *set* — which is what the filmstrip,
    /// Export All and Paste to All all need before they do anything.
    public func openFolder(_ url: URL) {
        flush()
        var count = 0
        run { count = try session.openFolder(url) }
        refresh()
        guard problem == nil else { return }
        notice = count == 1
            ? "opened 1 photograph"
            : "opened \(count) photographs"
    }

    public func openPaths(_ urls: [URL]) {
        flush()
        run { try session.openPaths(urls) }
        refresh()
    }

    /// Show a different photograph of the set, keeping the edit on the one
    /// being left.
    ///
    /// No `flush` first, unlike `open`: the engine writes the outgoing edit out
    /// as part of the switch, so a flush here would be a second write of the
    /// same document.
    public func focus(_ index: Int) {
        run { try session.focus(index) }
        refresh()
    }

    /// Whether there is a photograph after, or before, the one in hand.
    ///
    /// What the menu items are greyed by, and clamped rather than wrapping:
    /// `main.rs` clamps too, and a set that wraps quietly takes you back to the
    /// first photograph when you thought you were at the last.
    public var hasNext: Bool {
        guard let current = library.current else { return false }
        return current + 1 < library.count
    }

    public var hasPrevious: Bool { (library.current ?? 0) > 0 }

    /// The next photograph of the set, if there is one.
    public func showNext() {
        guard let current = library.current, hasNext else { return }
        focus(current + 1)
    }

    /// And the one before.
    public func showPrevious() {
        guard let current = library.current, hasPrevious else { return }
        focus(current - 1)
    }

    /// Ask for the thumbnails of the entries a strip is actually showing.
    ///
    /// The visible range, not the whole set: opening a folder of a thousand
    /// should not queue a thousand decodes before the first one anybody can
    /// see. Asking twice for the same entry costs nothing.
    public func requestThumbnails(_ range: Range<Int>) {
        session.requestThumbnails(range)
    }

    /// The picture for one entry, once its thumbnail has arrived.
    ///
    /// A dictionary lookup and nothing else. The copy out of the engine and the
    /// Core Graphics image are both made once, in `collectThumbnails`, so a
    /// strip may ask this of every visible entry on every body evaluation.
    public func thumbnail(for entry: LibraryEntry) -> CGImage? {
        thumbnails[entry.path]
    }

    public func thumbnail(at index: Int) -> CGImage? {
        guard let entry = library[index] else { return nil }
        return thumbnails[entry.path]
    }

    /// Take delivery of thumbnails, and copy across only what arrived.
    ///
    /// The engine's answer is the whole mechanism, the way the scope generation
    /// is for the counts: false means nothing has moved and nothing is copied.
    /// A thumbnail is 64 KB and a set can be two hundred of them, so a store
    /// that re-copied on a schedule would spend thirteen megabytes a frame to
    /// arrive at the pictures it already had.
    ///
    /// The answer says whether anything did arrive, which is the one thing
    /// worth knowing about a call made every frame.
    @discardableResult
    public func collectThumbnails() -> Bool {
        guard session.collectThumbnails() else { return false }
        // The marks moved with the delivery: `hasThumbnail` for what arrived,
        // `failed` for what could not be read.
        refreshLibrary()
        syncThumbnails()
        return true
    }

    /// Build a picture for every thumbnail that has one and this does not.
    ///
    /// Called only when `collectThumbnails` reports a delivery. Walking the
    /// whole set to find the new arrivals is a lookup per photograph, which is
    /// exactly the kind of thing a strip refuses to do per frame — but this is
    /// not per frame, it is per thumbnail, and a thumbnail is a decode.
    private func syncThumbnails() {
        for entry in library.entries where entry.hasThumbnail {
            guard thumbnails[entry.path] == nil else { continue }
            do {
                // Nil is a thumbnail that is not the shape it claims to be,
                // which is not something to keep asking about; a throw is the
                // engine and this side disagreeing about a buffer, which is.
                if let picture = try session.thumbnail(entry.index)?.image {
                    thumbnails[entry.path] = picture
                }
            } catch {
                // Set rather than run through `run`: a thumbnail turning up is
                // no reason to clear a refusal the person has not read yet.
                problem = String(describing: error)
            }
        }
    }

    /// Pull the set across, and write it only when it has actually moved.
    ///
    /// A session with no set at all — nothing open, or the built-in chart —
    /// answers in one C call. Otherwise it is a path and three marks per entry,
    /// which is what a strip needs and is not a picture. Never called per
    /// frame: the deliveries are, and a delivery is one call until something
    /// actually arrives.
    private func refreshLibrary() {
        let count = session.entryCount
        guard count > 0 || !library.isEmpty else { return }

        var entries: [LibraryEntry] = []
        entries.reserveCapacity(count)
        for index in 0..<count {
            guard let path = session.entryPath(index), let marks = session.entryFlags(index)
            else { continue }
            entries.append(
                LibraryEntry(
                    index: index, path: path, edited: marks.edited, failed: marks.failed,
                    hasThumbnail: marks.hasThumbnail))
        }

        let next = Library(entries: entries, current: session.currentEntry)
        // Only written when it is actually changing: assigning an equal set
        // would tell every observing view to run its body again for nothing.
        guard next != library else { return }
        library = next

        // A picture is 64 KB. A session worked through folder after folder
        // would otherwise end up holding one of every photograph it had ever
        // been shown, which is the accounting a filmstrip exists to avoid.
        let kept = Set(entries.map(\.path))
        if thumbnails.contains(where: { !kept.contains($0.key) }) {
            thumbnails = thumbnails.filter { kept.contains($0.key) }
        }
    }

    // ---- what is remembered between runs ----------------------------------
    //
    // The stars and the set that was open, which live in the engine rather than
    // in `@AppStorage` because they mean the same thing in both shells. What
    // stays in `@AppStorage` is per-window interface state: which tool is
    // showing, whether the scopes are open.

    /// The effects that have been starred.
    ///
    /// A stored property, so a browser's `body` reads a value rather than
    /// walking the ABI once per effect — the shelf asks about all thirty every
    /// time it is drawn. Written when the support directory is named, which is
    /// when the engine reads the settings file, and again after every toggle.
    public private(set) var favourites: [String] = []

    public func isFavourite(_ key: String) -> Bool {
        favourites.contains(key)
    }

    /// Star or unstar an effect. Written out by the engine as it happens.
    public func toggleFavourite(_ key: String) {
        run { try session.toggleFavourite(key) }
        refreshFavourites()
    }

    /// Pull the stars across, and write them only when they have moved.
    ///
    /// A refusal here is a null handle, which the store's own session cannot
    /// be, so there is nothing to put in the status bar.
    private func refreshFavourites() {
        guard let starred = try? session.favourites() else { return }
        if starred != favourites { favourites = starred }
    }

    /// Open the set that was open when this last ran, or the built-in chart.
    ///
    /// **This is the whole of the Mac's launch policy, and it exists because
    /// the engine cannot have one.** `Engine.rememberedSession` drops the
    /// photographs that are no longer there, which is as far as `is_file` can
    /// go: a file that is still there and will *not decode* comes back in the
    /// list, and opening it is a refusal. Somebody whose launch died on that
    /// refusal would have a window that never appears and no way out but
    /// finding and deleting a settings file — so:
    ///
    /// - Nothing remembered, or nothing left of it, opens the chart. That is a
    ///   first run and it is also a folder that has been tidied up.
    /// - A set opens whole if it can. Only the first photograph is decoded, so
    ///   only the first can refuse; if it does, it is dropped and the rest are
    ///   tried, and so on. A set of sixty whose first frame is corrupt opens on
    ///   the second, which is what `apps/windows/src/main.rs` does.
    /// - Nothing in the set opening at all falls back to the chart, with the
    ///   first refusal in the status bar.
    /// - Then the photograph that was showing is focused. If *it* is the one
    ///   that will not decode, the set stays where it opened and says so. A bad
    ///   photograph costs its own place in the set and nothing else.
    ///
    /// Every refusal goes to `problem`, unlike in the engine, because by now
    /// there is a window to say it in.
    ///
    /// Note what dropping means: the engine writes down what it actually
    /// opened, so a photograph that would not decode is not in the set the
    /// *next* launch tries either. That is the same outcome as one that was
    /// deleted, and it is undone by opening the folder again.
    public func openRemembered() {
        var paths: [URL] = []
        var showing = 0
        do {
            (paths, showing) = try session.rememberedSession()
        } catch {
            // A null handle, which this session cannot be. A launch is not the
            // place to insist on it.
            paths = []
        }

        // The first refusal rather than the last, because it is the one that
        // names the photograph the person was actually on.
        var trouble: String?
        while !paths.isEmpty {
            do {
                try session.openPaths(paths)
                break
            } catch {
                if trouble == nil { trouble = String(describing: error) }
                paths.removeFirst()
                // The remembered position counts from the front of the list
                // that is shrinking under it. Past the front it slides down
                // with everything else; at or before it, the photograph that
                // was showing is one of the ones that would not open.
                showing = showing > 0 ? showing - 1 : 0
            }
        }

        guard !paths.isEmpty else {
            openTestChart()
            // After the chart, not before: opening one succeeds, and `run`
            // clears `problem` on the way past.
            if trouble != nil { problem = trouble }
            return
        }

        if showing > 0 {
            do {
                try session.focus(showing)
            } catch {
                // The one that was showing will not decode. The set is still
                // open on its first photograph, which is a launch rather than
                // a failure to launch.
                if trouble == nil { trouble = String(describing: error) }
            }
        }
        refresh()
        // Written only when there is something to say, so that a refusal
        // `refresh` itself ran into is not quietly cleared by a launch that
        // went well otherwise.
        if trouble != nil { problem = trouble }
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

    /// Whether a hand is on the crop rectangle right now.
    ///
    /// Observed, which `dragging` deliberately is not: the thirds grid is a
    /// picture of the gesture, and since the gesture moved into the viewer the
    /// overlay has no state of its own to read it from.
    public private(set) var croppingByHand = false

    /// Bracket a crop drag: one undo step, and the grid while the hand is down.
    public func beginCropDrag() {
        beginInteraction("Crop")
        croppingByHand = true
    }

    public func endCropDrag() {
        croppingByHand = false
        endInteraction()
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

    /// Whether the viewer is showing the photograph with the stack switched
    /// off, which is what the Bypass control is bound to.
    ///
    /// Stored rather than read through to the engine, for the reason
    /// ``hasGrade`` is: `@Observable` tracks stored properties, and a toggle
    /// bound to a computed one that reaches through would keep whatever state
    /// it was built with.
    public private(set) var bypassAll = false

    /// Show the photograph with the whole stack switched off, or stop.
    ///
    /// Not an edit and not in the history: it is a way of *looking* at the
    /// picture, like the crop framing and the comparison. An export writes the
    /// grade whatever this is set to.
    public func setBypassAll(_ bypass: Bool) {
        run { try session.setBypassAll(bypass) }
        guard problem == nil else { return }
        bypassAll = bypass
    }

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

    // ---- comparing --------------------------------------------------------

    /// Which comparison the viewer is showing. A property of the window, like
    /// ``SessionStore/view`` and ``SessionStore/cropping`` — not of the
    /// photograph, and not in the history.
    public private(set) var compare: Compare = .off

    /// Where a wipe's seam sits, as a fraction of the viewer's width.
    ///
    /// Mirrored here rather than read through to the engine on every frame
    /// because the overlay draws it and a `body` that walks the ABI is a body
    /// that cannot be a pure function of observed state.
    ///
    /// Kept while the mode is off, exactly as the engine keeps it: cycling
    /// round to a wipe again puts the seam back where the user left it.
    ///
    /// Declared at zero and never observed there: `init` reads the engine's own
    /// answer, which is a half. Writing the half here as well would put the
    /// number in two places and leave this side quietly disagreeing with the
    /// engine the day it moved.
    public private(set) var wipe: CGFloat = 0

    /// The one button: off → wipe → side → off.
    ///
    /// The fraction is read back and handed straight to the next call. It is
    /// the one thing `pe_session_set_compare` lets a caller throw away — pass
    /// zero on the way past off and the next wipe begins at the left edge —
    /// so the cycle reads the pair rather than assuming its own mirror is the
    /// engine's.
    public func cycleCompare() {
        guard let held = try? session.compare() else { return }
        set(held.mode.next, wipe: held.wipe)
    }

    /// Move the seam, keeping the mode. What a drag on it calls, once a frame.
    public func setWipe(_ fraction: CGFloat) {
        set(compare, wipe: Float(fraction))
    }

    /// Show a comparison, or stop. For anything that names a mode rather than
    /// cycling — a menu item, a test.
    public func setCompare(_ mode: Compare) {
        set(mode, wipe: Float(wipe))
    }

    private func set(_ mode: Compare, wipe fraction: Float) {
        run { try session.setCompare(mode, wipe: fraction) }
        // The engine clamps, so what it stored is not always what it was
        // asked for. Read it back rather than mirroring the proposal, the same
        // rule the crop rectangle follows.
        refreshCompare()
    }

    /// Pull the mode and the seam across, and write them only when they moved.
    ///
    /// A refusal here is a null handle, which cannot happen — the session owns
    /// its own — so there is nothing to put in the status bar.
    private func refreshCompare() {
        guard let held = try? session.compare() else { return }
        if compare != held.mode { compare = held.mode }
        let fraction = CGFloat(held.wipe)
        if wipe != fraction { wipe = fraction }
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

    // ---- the explicit save -------------------------------------------------

    /// Write a `.peproj` beside the photograph.
    ///
    /// A sidecar is a decision — *this* is the edit, keep it, move it with the
    /// photograph. The autosave is only where you happened to stop, and it
    /// lives in Application Support rather than beside the file.
    ///
    /// Says where it went, because a file written silently somewhere the reader
    /// cannot see is one they cannot be sure about.
    public func saveEdit() {
        var written: URL?
        run { written = try session.saveSidecar() }
        guard problem == nil, let written else { return }
        notice = "saved \(written.lastPathComponent)"
    }

    /// A `.peproj` beside every photograph of the set that has an edit.
    ///
    /// Says both numbers when any were refused. A count on its own cannot tell
    /// you whether the run went well — "saved 40 edits" reads as success when
    /// nine of the forty-nine were skipped — so a run with failures is reported
    /// as a problem rather than as a notice.
    public func saveAllEdits() {
        var counts = (written: 0, failed: 0)
        run { counts = try session.saveAllSidecars() }
        guard problem == nil else { return }
        if counts.failed == 0 {
            notice = counts.written == 1 ? "saved 1 edit" : "saved \(counts.written) edits"
        } else {
            problem = "saved \(counts.written) edits, \(counts.failed) failed"
        }
    }

    /// Pull a sidecar back over whatever is showing, as one undo step.
    public func loadEdit(_ url: URL) {
        run { try session.loadSidecar(url) }
        refresh()
        guard problem == nil else { return }
        notice = "loaded \(url.lastPathComponent)"
    }

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

    // ---- a batch ----------------------------------------------------------
    //
    // Every photograph in the set, one per frame, from `renderIfNeeded` — the
    // same tick that collects thumbnails and drives the autosave. Sixty
    // photographs is sixty full-resolution renders: a loop would freeze the
    // window for a minute with no way to tell whether it was working or hung
    // and no way to stop it, and a step inside a `body` would be that same
    // freeze with a view update wrapped round it.

    /// How far the run has got, or nil when there is no run.
    ///
    /// Written only when the counts actually move, so a bar drawing them is not
    /// asked to run its body again sixty times a second for three numbers it
    /// already has.
    public private(set) var batch: BatchCounts?

    /// What the run that just ended did: `n exported`, or `n exported, m
    /// failed`. Nil until one ends, and again once it has been dismissed.
    ///
    /// Its own property rather than `problem`, and not only because finishing
    /// is not a failure: `problem` is cleared by the next call that succeeds,
    /// which on this path is the tick of the very next frame. A run that
    /// silently stops is indistinguishable from one that crashed, and this is
    /// the thing that tells them apart — which is the whole reason the engine
    /// keeps a finished run's counts until they are read.
    public private(set) var batchSummary: String?

    /// Whether a run could be started: there are photographs, and nothing is
    /// already running.
    public var canStartBatch: Bool { !library.isEmpty && batch == nil }

    /// Begin exporting every photograph in the set into `directory`.
    ///
    /// Somewhere chosen rather than beside each original: a batch written back
    /// into the folder it read would be the next run's input. Refused with no
    /// set open — the built-in chart is not a set of one — and the refusal
    /// arrives in `problem` like any other.
    public func startBatch(into directory: URL) {
        var started = false
        run {
            try session.startBatch(into: directory)
            started = true
        }
        guard started else { return }
        batchSummary = nil
        batch = session.batchProgress()
        if batch == nil {
            // Nothing in the ABI produces this: a run that started reports its
            // counts. Saying so beats a run that never steps and never says why.
            problem = "the engine started a run it does not have"
        }
    }

    /// Export one photograph, if a run is on. Called from the frame tick.
    ///
    /// The answer says whether a step was taken, which is the one thing worth
    /// knowing about a call made every frame.
    @discardableResult
    public func stepBatch() -> Bool {
        guard batch != nil else { return false }

        var more = false
        var refusal: String?
        do {
            more = try session.stepBatch()
        } catch {
            // A refusal is the engine having no device to render with, which
            // ends the whole run. A photograph that merely could not be written
            // is counted in `failed` and stepped past, and arrives here as an
            // ordinary step with one more in that count.
            refusal = String(describing: error)
        }

        // Read after the step and before the run is put away. A finished run
        // keeps its counts until it is cancelled, which is the whole reason the
        // summary below can be written at all.
        let counts = session.batchProgress()
        if counts != batch { batch = counts }

        if let refusal {
            // In `problem` too, for the frame it survives; the summary is the
            // copy that is still there when somebody looks up.
            problem = refusal
            end(saying: "stopped after \(Self.tally(counts)): \(refusal)")
        } else if !more {
            end(saying: Self.tally(counts))
        }
        return true
    }

    /// Stop the run, keeping what it has already written.
    ///
    /// Nothing is taken back: half a folder of exports is the state somebody
    /// asked for when they pressed stop.
    public func cancelBatch() {
        guard batch != nil else { return }
        // The engine's counts rather than the held copy: this is a button
        // press, not a frame, and the run is still there to be asked.
        end(saying: "stopped after \(Self.tally(session.batchProgress()))")
    }

    /// Put the summary away, once it has been read.
    public func dismissBatchSummary() {
        if batchSummary != nil { batchSummary = nil }
    }

    /// Say what the run did, and let the engine put its counts away.
    private func end(saying said: String) {
        batchSummary = said
        session.cancelBatch()
        batch = nil
    }

    /// `n exported`, or `n exported, m failed`.
    private static func tally(_ counts: BatchCounts?) -> String {
        guard let counts else { return "nothing exported" }
        return counts.failed == 0
            ? "\(counts.done) exported"
            : "\(counts.done) exported, \(counts.failed) failed"
    }

    // ---- the mechanism ----------------------------------------------------

    /// Pull the engine's state across, but only when it has actually moved.
    ///
    /// The version is one integer; the snapshot is a document. Comparing the
    /// former before decoding the latter is what makes mirroring cheap enough
    /// to do after every structural edit.
    private func refresh() {
        // An edit supersedes whatever the last command had to say. Here rather
        // than in `run`, which the render loop also goes through — a notice
        // cleared on the next frame would never be read.
        notice = nil
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
        // Before the version check for the same reason: which photograph of the
        // set is on screen is not in the snapshot, and neither is the edited
        // mark on the one just switched away from.
        refreshLibrary()
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
            // A refusal is not a result. Leaving the last success on screen
            // beside a fresh failure reads as though both just happened.
            notice = nil
        }
    }

    // ---- the grade in hand -------------------------------------------------

    /// Copy this photograph's grade, to put on another.
    public func copyGrade() {
        run { try session.copyGrade() }
        guard problem == nil else { return }
        hasGrade = true
        notice = "grade copied"
    }

    /// Whether there is a grade to paste, which the Paste items are greyed by.
    ///
    /// **Stored, not asked of the engine on every read.** `@Observable` tracks
    /// stored properties; a computed one that reaches through to the session is
    /// invisible to it, so a menu item bound to it keeps whatever enabled state
    /// it was built with — Paste stayed grey after a copy, with the status bar
    /// saying "grade copied" right beside it.
    ///
    /// Safe to mirror because [`copyGrade`] is the only thing that fills the
    /// engine's clipboard, and nothing empties it: a grade in hand stays in
    /// hand for the sitting.
    public private(set) var hasGrade = false

    /// Put the copied grade on this photograph.
    ///
    /// No notice: the picture changes, and that is the notice.
    public func pasteGrade() {
        run { try session.pasteGrade() }
        refresh()
    }

    /// Put it on every *other* photograph in the set.
    ///
    /// This one does say so. Nothing on screen changes — the photograph in hand
    /// is deliberately not one of them — so silence would be indistinguishable
    /// from the command having failed.
    public func pasteGradeToAll() {
        var count = 0
        run { count = try session.pasteGradeToAll() }
        guard problem == nil else { return }
        notice = count == 1 ? "grade pasted to 1 photo" : "grade pasted to \(count) photos"
    }
}
