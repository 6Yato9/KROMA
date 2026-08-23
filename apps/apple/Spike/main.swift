// The smallest thing that can tell us whether the layer path works.
//
// A window, a layer-backed view, and one call into Rust asking it to fill the
// layer. If this window comes up orange, wgpu built a surface on a CAMetalLayer
// that Swift created, and the viewer decision in the spec holds.

import AppKit
import QuartzCore

final class SpikeView: NSView {
    private var attached = false

    override func makeBackingLayer() -> CALayer { CAMetalLayer() }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .duringViewResize
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard !attached, let metal = layer as? CAMetalLayer else { return }
        metal.contentsScale = window?.backingScaleFactor ?? 2.0
        metal.drawableSize = CGSize(
            width: bounds.width * metal.contentsScale,
            height: bounds.height * metal.contentsScale
        )
        let ptr = Unmanaged.passUnretained(metal).toOpaque()
        let rc = pe_spike_attach_and_clear(
            ptr,
            UInt32(metal.drawableSize.width),
            UInt32(metal.drawableSize.height)
        )
        attached = true
        if rc != 0 {
            print("attach failed, rc=\(rc)")
            NSApp.terminate(nil)
        } else {
            print("attached and cleared")
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)

let window = NSWindow(
    contentRect: NSRect(x: 0, y: 0, width: 640, height: 400),
    styleMask: [.titled, .closable],
    backing: .buffered,
    defer: false
)
window.title = "Kroma spike"
window.contentView = SpikeView(frame: window.contentRect(forFrameRect: window.frame))
window.center()
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
app.run()
