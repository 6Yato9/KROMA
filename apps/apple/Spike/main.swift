// The smallest thing that can tell us whether the layer path works.
//
// A window, a layer-backed view, and the engine's own session driving it. If
// this window shows the test chart, the whole pipeline crossed the boundary:
// decode, working space, stack, display transform, and a Metal layer Swift
// created.

import AppKit
import QuartzCore

/// The engine's last complaint, or nil. The engine allocated the string, so
/// the engine frees it.
private func lastError(_ session: OpaquePointer?) -> String? {
    guard let raw = pe_session_last_error(session) else { return nil }
    defer { pe_string_free(raw) }
    return String(cString: raw)
}

final class SpikeView: NSView {
    private var attached = false
    /// Held for the life of the view: the layer must outlive the attachment,
    /// and `detach_layer` has to run before either goes away.
    private var session: OpaquePointer?

    override func makeBackingLayer() -> CALayer { CAMetalLayer() }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .duringViewResize
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    deinit {
        pe_session_detach_layer(session)
        pe_session_free(session)
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard !attached, let metal = layer as? CAMetalLayer else { return }
        metal.contentsScale = window?.backingScaleFactor ?? 2.0
        metal.drawableSize = CGSize(
            width: bounds.width * metal.contentsScale,
            height: bounds.height * metal.contentsScale
        )
        let ptr = Unmanaged.passUnretained(metal).toOpaque()

        let session = pe_session_new()
        self.session = session
        let rc = pe_session_attach_layer(
            session,
            ptr,
            UInt32(metal.drawableSize.width),
            UInt32(metal.drawableSize.height)
        )
        if rc == 0 {
            let opened = pe_session_open_test_chart(session, 512, 512)
            let drawn = pe_session_render(session)
            print("attach=\(rc) open=\(opened) render=\(drawn)")
            print("rows=\(pe_session_row_count(session)) passes=\(pe_session_last_passes(session))")
            print("needs_render=\(pe_session_needs_render(session))")
            if opened != 0 || drawn != 0 {
                print("engine said: \(lastError(session) ?? "nothing")")
                NSApp.terminate(nil)
            }
        } else {
            print("attach failed, rc=\(rc): \(lastError(session) ?? "no message")")
            NSApp.terminate(nil)
        }
        attached = true
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
