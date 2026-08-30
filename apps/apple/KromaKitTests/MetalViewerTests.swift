import AppKit
import CoreGraphics
import SwiftUI
import XCTest

// Same module as the code under test; see EngineTests.swift.

/// Who gets the pointer when something is drawn over the viewer.
///
/// The crop overlay was a SwiftUI layer in front of `MetalViewerView` with a
/// drag gesture on it, and the comment beside it said the gesture "takes the
/// drag before the viewer's own pan sees it". It took rather more than that.
/// `NSHostingView` answers AppKit's `hitTest` for any SwiftUI content that is
/// hit-testable under the point — a bare `Canvas` with no gesture at all is
/// enough — and the hosting view is `MetalViewerView`'s **ancestor**, so an
/// event that lands on it runs *up* the responder chain and never reaches the
/// viewer below. Zoom, pan and double-click to fit all stopped the moment the
/// crop tool opened, and nothing could fail on it: a picture that will not zoom
/// looks exactly like a picture nobody has zoomed.
///
/// So the two overlays draw and the viewer decides, and this is the measurement
/// that says so. It is an assertion about AppKit's own dispatch rather than
/// about SwiftUI's documented behaviour, which is why the last case here builds
/// the *broken* layering and checks that it really is broken — a hit test that
/// answered the viewer whatever was over it would make every case above pass
/// while proving nothing.
final class MetalViewerTests: XCTestCase {

    private static let size = CGSize(width: 400, height: 300)

    /// Put a view in a real window, lay it out, and hand back the window, the
    /// hosting view and the viewer inside it.
    @MainActor
    private static func hosted<V: View>(_ view: V) -> (
        window: NSWindow, host: NSView, viewer: MetalViewerView?
    ) {
        let host = NSHostingView(rootView: AnyView(view))
        host.frame = CGRect(origin: .zero, size: size)
        let window = NSWindow(
            contentRect: host.frame, styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        window.layoutIfNeeded()
        return (window, host, find(host))
    }

    private static func find(_ view: NSView) -> MetalViewerView? {
        if let viewer = view as? MetalViewerView { return viewer }
        for sub in view.subviews {
            if let found = find(sub) { return found }
        }
        return nil
    }

    /// The points the check is made at: the middle, and each corner well inside
    /// the edge. A crop rectangle covers the middle and a wipe's seam runs
    /// through it, so an overlay that claimed only what it drew would still
    /// pass a check made at one corner.
    private static let points = [
        CGPoint(x: 200, y: 150), CGPoint(x: 8, y: 8), CGPoint(x: 392, y: 8),
        CGPoint(x: 8, y: 292), CGPoint(x: 392, y: 292),
    ]

    @MainActor
    private static func opened() throws -> SessionStore {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        return store
    }

    /// With nothing over it, the viewer answers. The baseline every case below
    /// is measured against.
    @MainActor
    func testTheBareViewerGetsThePointer() throws {
        let store = try Self.opened()
        let (_, host, viewer) = Self.hosted(
            MetalViewer(store: store).frame(width: Self.size.width, height: Self.size.height))
        defer { viewer?.stop() }
        let found = try XCTUnwrap(viewer, "no MetalViewerView was ever made")
        for at in Self.points {
            XCTAssertTrue(
                host.hitTest(at) === found,
                "the bare viewer did not answer at \(at) — this test is measuring nothing")
        }
    }

    /// The crop overlay over it, which is the case that was broken.
    @MainActor
    func testTheCropOverlayLeavesThePointerToTheViewer() throws {
        let store = try Self.opened()
        store.setCropping(true)
        let (_, host, viewer) = Self.hosted(
            MetalViewer(store: store)
                .frame(width: Self.size.width, height: Self.size.height)
                .overlay { CropOverlay(store: store) })
        defer { viewer?.stop() }
        let found = try XCTUnwrap(viewer)
        for at in Self.points {
            XCTAssertTrue(
                host.hitTest(at) === found,
                "the crop overlay took the pointer at \(at): AppKit will send the wheel and "
                    + "the press to \(String(describing: type(of: host.hitTest(at)))) instead")
        }
    }

    /// And both of them at once, which is what the window actually draws while
    /// the crop tool is open and a comparison is on.
    @MainActor
    func testBothOverlaysTogetherLeaveThePointerToTheViewer() throws {
        let store = try Self.opened()
        store.setCropping(true)
        store.setCompare(.wipe)
        let (_, host, viewer) = Self.hosted(
            MetalViewer(store: store)
                .frame(width: Self.size.width, height: Self.size.height)
                .overlay {
                    ZStack {
                        CropOverlay(store: store)
                        CompareOverlay(store: store)
                    }
                })
        defer { viewer?.stop() }
        let found = try XCTUnwrap(viewer)
        for at in Self.points {
            XCTAssertTrue(
                host.hitTest(at) === found, "an overlay took the pointer at \(at)")
        }
    }

    /// The composition `ContentView` actually draws: both overlays *and* the
    /// drop destination that lets an effect be dragged onto the picture.
    ///
    /// A `dropDestination` registers a dragging destination rather than a mouse
    /// handler, so it should leave the wheel, the drag and the double-click to
    /// the viewer. "Should" is what the crop overlay did too, and it took all
    /// three. Every case above composes the overlays *without* the drop target,
    /// so without this one it is the single layer nothing checks — and the only
    /// way left to find out would be to open the application and try it.
    @MainActor
    func testTheDropDestinationLeavesThePointerToTheViewer() throws {
        let store = try Self.opened()
        // The hardest arrangement: the crop tool open and a comparison running,
        // with the drop target over both.
        store.setCropping(true)
        store.setCompare(.wipe)
        let (_, host, viewer) = Self.hosted(
            MetalViewer(store: store)
                .frame(width: Self.size.width, height: Self.size.height)
                .overlay {
                    ZStack {
                        CropOverlay(store: store)
                        CompareOverlay(store: store)
                    }
                }
                .dropDestination(for: DraggedEffect.self) { _, _ in true }
                // And the window's own, which takes a dropped photograph. Two
                // dragging destinations over the viewer, which is what
                // `ContentView` actually draws.
                .dropDestination(for: URL.self) { _, _ in true })
        defer { viewer?.stop() }
        let found = try XCTUnwrap(viewer)
        for at in Self.points {
            XCTAssertTrue(
                host.hitTest(at) === found,
                "a drop target took the pointer at \(at): the wheel, the pan and the "
                    + "double-click would all go to it instead of to the picture")
        }
    }

    /// The check on the check: the layering this replaced really does take the
    /// pointer away, so the cases above are not passing for free.
    ///
    /// A plain `Canvas` with no gesture and no `contentShape` — less than the
    /// crop overlay ever had — is enough to do it.
    @MainActor
    func testAnOverlayThatDoesTakeTheHitTestIsSeenToTakeIt() throws {
        let store = try Self.opened()
        let (_, host, viewer) = Self.hosted(
            MetalViewer(store: store)
                .frame(width: Self.size.width, height: Self.size.height)
                .overlay {
                    Canvas { context, _ in
                        context.stroke(
                            Path(CGRect(x: 0, y: 0, width: 10, height: 10)),
                            with: .color(.white))
                    }
                })
        defer { viewer?.stop() }
        let found = try XCTUnwrap(viewer)
        XCTAssertFalse(
            host.hitTest(CGPoint(x: 200, y: 150)) === found,
            "a hit-testing overlay left the pointer with the viewer, so the assertions "
                + "above cannot tell a working overlay from a broken one")
    }

    // ---- what a press takes hold of ---------------------------------------
    //
    // `main.rs` writes the whole rule as one line — `let pan = !self.cropping
    // && !self.dragging_wipe` — and this is that line, arranged so the three
    // answers are named rather than implied by two booleans.

    private static let viewer = CGSize(width: 400, height: 300)
    private static let whole = CGRect(x: 0, y: 0, width: 1, height: 1)

    private static func grab(
        _ point: CGPoint, cropping: Bool = false, crop: CGRect = whole,
        compare: Compare = .off, wipe: CGFloat = 0.5
    ) -> ViewerDrag {
        ViewerDrag.at(
            point, in: viewer, cropping: cropping, crop: crop, visible: whole,
            compare: compare, wipe: wipe)
    }

    func testWithNoToolAndNoComparisonEveryPressIsAPan() {
        for at in Self.points {
            XCTAssertEqual(Self.grab(at), .pan, "a plain press at \(at) was not a pan")
        }
    }

    /// A wipe's seam is grabbed from near it and nowhere else, so the drag that
    /// moves the seam does not cost the picture its pan.
    func testOnlyAPressNearTheSeamTakesTheSeam() {
        // The seam of a half wipe on a viewer 400 wide is at 200.
        XCTAssertEqual(Self.grab(CGPoint(x: 200, y: 150), compare: .wipe), .wipe)
        XCTAssertEqual(
            Self.grab(CGPoint(x: 200 + CompareGeometry.grab - 1, y: 150), compare: .wipe),
            .wipe, "just inside the grab distance is not the seam")
        XCTAssertEqual(
            Self.grab(CGPoint(x: 200 + CompareGeometry.grab + 1, y: 150), compare: .wipe),
            .pan, "a press a whole grab distance from the seam still took it")
        XCTAssertEqual(
            Self.grab(CGPoint(x: 40, y: 150), compare: .wipe), .pan,
            "the far side of the picture took the seam")
        // And the seam moves with the fraction, rather than sitting in the
        // middle whatever the wipe says.
        XCTAssertEqual(Self.grab(CGPoint(x: 100, y: 150), compare: .wipe, wipe: 0.25), .wipe)
        XCTAssertEqual(Self.grab(CGPoint(x: 200, y: 150), compare: .wipe, wipe: 0.25), .pan)
    }

    /// Side by side has no seam to drag, so it leaves the drag alone.
    func testASideBySideDoesNotTakeAnyDrag() {
        for at in Self.points {
            XCTAssertEqual(Self.grab(at, compare: .side), .pan, "side by side took \(at)")
        }
    }

    /// While the crop tool is open the rectangle has the drag, and a press that
    /// caught none of it does nothing at all — `main.rs`'s `!self.cropping`.
    /// The tool is showing the enclosing frame, so the hand is on it to move a
    /// rectangle rather than the picture under it.
    func testTheCropToolTakesTheDragAndAPressOnNothingDoesNotPan() {
        let crop = CGRect(x: 0.25, y: 0.25, width: 0.5, height: 0.5)
        // The middle of the rectangle, which lands at (200, 150) on screen.
        XCTAssertEqual(
            Self.grab(CGPoint(x: 200, y: 150), cropping: true, crop: crop),
            .crop(.move))
        // Its top-left corner, at (100, 75).
        XCTAssertEqual(
            Self.grab(CGPoint(x: 100, y: 75), cropping: true, crop: crop),
            .crop(.edge(left: true, right: false, top: true, bottom: false)))
        // And well outside it.
        XCTAssertEqual(
            Self.grab(CGPoint(x: 8, y: 8), cropping: true, crop: crop), .nothing,
            "a press outside the crop rectangle panned the picture out from under the tool")
    }

    /// The crop tool answers before the seam does. Both can be on — a
    /// comparison is a property of the window and survives changing tools — and
    /// the rectangle being edited is the one under the hand.
    func testACropGripBeatsASeamUnderIt() {
        let crop = CGRect(x: 0.25, y: 0.25, width: 0.5, height: 0.5)
        XCTAssertEqual(
            Self.grab(
                CGPoint(x: 200, y: 150), cropping: true, crop: crop, compare: .wipe),
            .crop(.move),
            "the seam took a drag that had hold of the crop rectangle")
    }
}
