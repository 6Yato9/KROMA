import SwiftUI
import QuartzCore

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

    deinit {
        link?.invalidate()
    }
}
