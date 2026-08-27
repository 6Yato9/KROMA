import SwiftUI
import QuartzCore

// -----------------------------------------------------------------------------
// What a press on the viewer has hold of
// -----------------------------------------------------------------------------

/// Which of the viewer's drags a press starts — `main.rs`'s
/// `let pan = !self.cropping && !self.dragging_wipe`, decided once and in one
/// place.
///
/// **This is why the overlays draw and nothing more.** A SwiftUI layer in front
/// of `MetalViewerView` takes the AppKit hit test away from it: `NSHostingView`
/// answers `hitTest` for any SwiftUI content that is hit-testable at that
/// point — a bare `Canvas` with no gesture on it is enough — and the hosting
/// view is `MetalViewerView`'s *ancestor*, so the responder chain from there
/// runs up and out of the window rather than down into the viewer. Every scroll
/// and every press the overlay covered stopped: zoom, pan and double-click to
/// fit all died the moment the crop tool opened, silently, because a picture
/// that will not zoom looks exactly like a picture nobody has zoomed.
///
/// So the overlays are `allowsHitTesting(false)` and the viewer decides. That
/// is measured rather than argued in `MetalViewerTests`.
public enum ViewerDrag: Equatable {
    /// A press that has hold of nothing, which is what a drag anywhere on the
    /// picture is while the crop tool is open. `main.rs` turns panning off for
    /// the whole gesture there; the tool is showing the enclosing frame so the
    /// hand is on it to move a rectangle, not the picture under it.
    case nothing
    /// The picture moves under the pointer.
    case pan
    /// One part of the crop rectangle.
    case crop(CropGrip)
    /// A wipe's seam.
    case wipe

    /// What a press at `point`, in the viewer's own points with the origin top
    /// left, takes hold of.
    ///
    /// The crop tool wins before the seam wins before the pan, and the order is
    /// the tool's: a comparison is a way of looking and a crop is an edit in
    /// progress, so the rectangle under the hand answers first.
    public static func at(
        _ point: CGPoint,
        in size: CGSize,
        cropping: Bool,
        crop: CGRect,
        visible: CGRect,
        compare: Compare,
        wipe: CGFloat
    ) -> ViewerDrag {
        if cropping {
            let onScreen = CropOverlay.place(
                crop, in: CGRect(origin: .zero, size: size), showing: visible)
            guard let grip = CropGrip.at(point, in: onScreen) else { return .nothing }
            return .crop(grip)
        }
        if compare == .wipe,
            CompareGeometry.grabsSeam(point.x, wipe: wipe, width: size.width)
        {
            return .wipe
        }
        return .pan
    }
}

/// The photograph, drawn by the engine into a layer this view owns.
///
/// Swift owns the view hierarchy and hands the engine a `CAMetalLayer`; the
/// engine builds a wgpu surface on it and presents. Every line of GPU code
/// stays in Rust, shared with the Windows shell — which is the rule that stops
/// the Mac port quietly becoming a second renderer.
///
/// Rendering is on demand. The display link ticks, the store is asked whether
/// anything moved, and nothing is drawn if nothing did. An editor that redraws
/// a hundred and twenty times a second while the user reads the histogram is a
/// laptop with a warm keyboard and an hour less battery.
public struct MetalViewer: NSViewRepresentable {
    private let store: SessionStore

    public init(store: SessionStore) {
        self.store = store
    }

    public func makeNSView(context: Context) -> MetalViewerView {
        MetalViewerView(store: store)
    }

    public func updateNSView(_ view: MetalViewerView, context: Context) {
        // Nothing to push. The view pulls from the store on its own tick, and
        // pushing here would render on SwiftUI's schedule rather than the
        // display's.
    }

    public static func dismantleNSView(_ view: MetalViewerView, coordinator: ()) {
        view.stop()
    }
}

/// The layer-backed view itself.
public final class MetalViewerView: NSView {
    private let store: SessionStore
    private var link: CADisplayLink?
    private var attached = false

    init(store: SessionStore) {
        self.store = store
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .duringViewResize
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not loaded from a nib") }

    public override func makeBackingLayer() -> CALayer { CAMetalLayer() }

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard window != nil else {
            stop()
            return
        }
        attachIfNeeded()
        start()
    }

    /// The drawable size in *pixels*, which is what the engine configures its
    /// surface with. Points would give a soft picture on every Retina display.
    private var drawableSize: (UInt32, UInt32) {
        let scale = window?.backingScaleFactor ?? 2
        return (
            UInt32(max(1, bounds.width * scale)),
            UInt32(max(1, bounds.height * scale))
        )
    }

    private func attachIfNeeded() {
        guard !attached, let metal = layer as? CAMetalLayer else { return }
        let scale = window?.backingScaleFactor ?? 2
        metal.contentsScale = scale
        let (w, h) = drawableSize
        metal.drawableSize = CGSize(width: CGFloat(w), height: CGFloat(h))
        store.attach(layer: metal, width: w, height: h)
        attached = true
    }

    public override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        guard attached, let metal = layer as? CAMetalLayer else { return }
        let (w, h) = drawableSize
        metal.drawableSize = CGSize(width: CGFloat(w), height: CGFloat(h))
        store.resize(width: w, height: h)
    }

    private func start() {
        guard link == nil else { return }
        // macOS 14's display link, which follows the screen the window is on
        // rather than a fixed sixty.
        let link = displayLink(target: self, selector: #selector(tick))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    func stop() {
        link?.invalidate()
        link = nil
        if attached {
            store.detachLayer()
            attached = false
        }
    }

    @objc private func tick() {
        store.renderIfNeeded()
    }

    // ---- the gestures ----------------------------------------------------
    //
    // All of them, for the whole viewer: the overlays draw and this view
    // decides what the pointer is doing. See `ViewerDrag` for why.

    /// What the press in flight has hold of, decided once when it started.
    /// Re-deciding every frame would hand a crop drag to whichever handle
    /// happened to pass under the pointer.
    private var held: ViewerDrag = .nothing
    /// Where the press started and the rectangle it started against, both in
    /// the frame's uv, plus the ratio a locked crop has to keep.
    ///
    /// Fixed for the length of the gesture: `CropGrip.dragged` measures its
    /// delta from where the drag started against the rectangle as it was then,
    /// because the engine corrects every proposal and accumulating would fold
    /// each correction into the next frame's starting point.
    private var press: (at: CGPoint, crop: CGRect, ratio: Double?)?

    /// The pointer in the view's own points with the origin top left, which is
    /// the frame every overlay and every crop rectangle is written in.
    ///
    /// The view is not `isFlipped`, and deliberately: it is backed by the
    /// `CAMetalLayer` the engine presents into, and flipping a layer-backed
    /// view is a change to what the engine draws rather than to how this reads
    /// a mouse.
    private func topLeft(_ event: NSEvent) -> CGPoint {
        let point = convert(event.locationInWindow, from: nil)
        return CGPoint(x: point.x, y: bounds.height - point.y)
    }

    public override func scrollWheel(with event: NSEvent) {
        // Scroll zooms, anchored under the cursor, which is what every editor
        // that is any good does and what the Windows shell does. It keeps
        // working while a tool is open for the reason `main.rs` gives: zooming
        // is the wheel and panning is a drag, and the whole point of their
        // being two controls is that a tool can take one and leave the other.
        guard bounds.width > 0, bounds.height > 0 else { return }
        let point = topLeft(event)
        let anchor = CGPoint(x: point.x / bounds.width, y: point.y / bounds.height)
        let factor = 1 + event.scrollingDeltaY * 0.01
        store.zoom(by: factor, at: anchor)
    }

    public override func mouseDown(with event: NSEvent) {
        // Double-click fits, as it does on the Windows side.
        if event.clickCount == 2 {
            store.fitView()
            return
        }
        guard bounds.width > 0, bounds.height > 0 else { return }
        let at = topLeft(event)
        let crop = store.cropRect
        held = ViewerDrag.at(
            at, in: bounds.size,
            cropping: store.cropping, crop: crop, visible: store.view.region,
            compare: store.compare, wipe: store.wipe)
        press = (
            at, crop,
            CropOverlay.screenRatio(of: store.geometry.aspect, showing: crop)
        )
        // A crop drag is an edit and collapses into one undo step; moving a
        // seam is not an edit at all, and neither is a pan.
        if case .crop = held { store.beginCropDrag() }
    }

    public override func mouseDragged(with event: NSEvent) {
        guard bounds.width > 0, bounds.height > 0, let press else { return }
        switch held {
        case .nothing:
            return
        case .pan:
            store.pan(
                by: CGSize(
                    width: event.deltaX / bounds.width,
                    height: -event.deltaY / bounds.height
                ))
        case .wipe:
            store.setWipe(
                CompareGeometry.fraction(ofX: topLeft(event).x, width: bounds.width))
        case let .crop(grip):
            // Screen points into the frame's uv. Zoomed in, a point on screen
            // is a smaller step across the frame, which is what `visible`
            // carries.
            let visible = store.view.region
            let at = topLeft(event)
            let delta = CGSize(
                width: (at.x - press.at.x) / bounds.width * visible.width,
                height: (at.y - press.at.y) / bounds.height * visible.height
            )
            store.setCropRect(
                CropGrip.dragged(press.crop, grip: grip, by: delta, ratio: press.ratio))
        }
    }

    public override func mouseUp(with event: NSEvent) {
        if case .crop = held { store.endCropDrag() }
        held = .nothing
        press = nil
    }

    public override var acceptsFirstResponder: Bool { true }

    deinit {
        link?.invalidate()
    }
}
