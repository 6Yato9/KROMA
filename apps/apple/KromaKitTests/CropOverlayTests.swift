import CoreGraphics
import SwiftUI
import XCTest

// Same module as the code under test; see EngineTests.swift.

/// The crop overlay: where the rectangle sits, what a drag does to it, and what
/// the thing actually looks like.
///
/// Three parts, and they fail for different reasons.
///
/// The first asks a real engine where the crop is. It used to pin a *copy* of
/// `Geometry::enclosing` and `Geometry::crop_uv_in` that lived in `CropFrame`,
/// against numbers transcribed from `pe_core` — a fixture in all but name,
/// because the C ABI carried the seven scalars of a geometry and nothing that
/// answered "where is the crop in the frame you are showing me". It answers that
/// now (`pe_session_crop_in_frame`), the copy is gone, and so are the
/// transcribed numbers: what is left to check on this side is not the
/// arithmetic — which cannot drift from an engine that performs it — but that
/// the overlay asks and draws the answer.
///
/// The second is the drag, which is this side's: which handle a press catches,
/// what a delta does to a rectangle, and where that rectangle lands on screen.
///
/// The third renders the overlay headlessly and reads the bitmap back, the way
/// `RowMetricsTests`, `CurveBackdropTests` and `WarperCloudTests` do. What that
/// can check is where the ink lands and how heavy it is. Whether the result is
/// *pleasant* — and whether the rectangle lands on the photograph when the
/// engine is drawing it into a Metal layer underneath — stays unverified until
/// somebody looks at it.
final class CropOverlayTests: XCTestCase {

    private static let source = CGSize(width: 1024, height: 768)

    /// The crop every case below is a variation of.
    private static func base(
        angle: Double = 0, turns: Int = 0, flipH: Bool = false, flipV: Bool = false,
        aspect: AspectLock = .free
    ) -> GeometryValue {
        GeometryValue(
            centre: CGPoint(x: 0.1, y: -0.05),
            size: CGSize(width: 0.5, height: 0.4),
            angle: angle, turns: turns, flipH: flipH, flipV: flipV, aspect: aspect
        )
    }

    /// The six geometries the mapping used to be pinned against, one of each
    /// shape of trouble: a plain crop, a straightened one, a quarter-turn, a
    /// turn with a flip, and both flips at a negative angle. The permutation is
    /// the part that looks entirely plausible when it is wrong.
    private static var cases: [(name: String, crop: GeometryValue)] {
        [
            ("plain", base()),
            ("angle 12", base(angle: 12)),
            ("one quarter-turn", base(turns: 1)),
            ("one quarter-turn, flipped horizontally", base(turns: 1, flipH: true)),
            (
                "three quarter-turns, flipped vertically, 7.5 degrees",
                base(angle: 7.5, turns: 3, flipV: true)
            ),
            (
                "two quarter-turns, both flips, -20 degrees",
                base(angle: -20, turns: 2, flipH: true, flipV: true)
            ),
        ]
    }

    /// A session with the test chart open and a geometry set, ready to be asked
    /// where the crop is.
    @MainActor
    private static func opened(_ crop: GeometryValue, cropping: Bool) throws -> SessionStore {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: UInt32(source.width), height: UInt32(source.height))
        store.setGeometry(crop)
        store.setCropping(cropping)
        XCTAssertNil(store.problem)
        return store
    }

    // ---- where the engine says the crop is --------------------------------

    /// With the tool closed the viewer is showing the crop itself, so the crop
    /// fills it. Six geometries, and the one call answers all six without this
    /// side knowing anything about turns or flips.
    @MainActor
    func testTheCropFillsTheFrameUntilTheToolIsOpened() throws {
        for c in Self.cases {
            let store = try Self.opened(c.crop, cropping: false)
            let r = store.cropRect
            XCTAssertEqual(Double(r.minX), 0, accuracy: 1e-3, "\(c.name): min x")
            XCTAssertEqual(Double(r.minY), 0, accuracy: 1e-3, "\(c.name): min y")
            XCTAssertEqual(Double(r.width), 1, accuracy: 1e-3, "\(c.name): width")
            XCTAssertEqual(Double(r.height), 1, accuracy: 1e-3, "\(c.name): height")
        }
    }

    /// And opening the tool puts the crop *inside* a bigger frame, which is the
    /// property the whole tool rests on: there is something outside the
    /// rectangle to see, and to drag back into.
    @MainActor
    func testOpeningTheToolPutsTheCropInsideABiggerFrame() throws {
        for c in Self.cases {
            let store = try Self.opened(c.crop, cropping: true)
            let r = store.cropRect
            XCTAssertGreaterThanOrEqual(Double(r.minX), -1e-3, "\(c.name): off the left")
            XCTAssertGreaterThanOrEqual(Double(r.minY), -1e-3, "\(c.name): off the top")
            XCTAssertLessThanOrEqual(Double(r.maxX), 1 + 1e-3, "\(c.name): off the right")
            XCTAssertLessThanOrEqual(Double(r.maxY), 1 + 1e-3, "\(c.name): off the bottom")
            XCTAssertLessThan(
                Double(r.width * r.height), 0.9,
                "\(c.name): the crop fills the frame the tool is showing, so there is "
                    + "nothing outside it to drag back in")
        }
    }

    /// The one case anybody can check by hand: half the frame, dead centre,
    /// unstraightened. The frame is then the whole source and the crop is the
    /// middle half of it.
    @MainActor
    func testACentredHalfCropIsTheMiddleOfTheFrame() throws {
        let store = try Self.opened(
            GeometryValue(
                centre: .zero, size: CGSize(width: 0.5, height: 0.5), angle: 0, turns: 0,
                flipH: false, flipV: false, aspect: .free),
            cropping: true)
        XCTAssertEqual(Double(store.cropRect.minX), 0.25, accuracy: 2e-3)
        XCTAssertEqual(Double(store.cropRect.minY), 0.25, accuracy: 2e-3)
        XCTAssertEqual(Double(store.cropRect.width), 0.5, accuracy: 2e-3)
        XCTAssertEqual(Double(store.cropRect.height), 0.5, accuracy: 2e-3)
    }

    /// An uncropped photograph fills its frame with the tool open too — the
    /// case the overlay opens on, and the one where a sign error is invisible
    /// everywhere else.
    @MainActor
    func testAnUncroppedPhotographFillsTheFrame() throws {
        let store = try Self.opened(.identity, cropping: true)
        XCTAssertEqual(Double(store.cropRect.minX), 0, accuracy: 1e-3)
        XCTAssertEqual(Double(store.cropRect.minY), 0, accuracy: 1e-3)
        XCTAssertEqual(Double(store.cropRect.width), 1, accuracy: 1e-3)
        XCTAssertEqual(Double(store.cropRect.height), 1, accuracy: 1e-3)
    }

    /// Reading the rectangle and writing it back is the round trip every drag
    /// makes, so a mismatch here moves the crop a little on every frame of a
    /// gesture that was not supposed to move it at all.
    @MainActor
    func testPuttingTheRectangleBackWhereItIsChangesNothing() throws {
        for c in Self.cases {
            let store = try Self.opened(c.crop, cropping: true)
            let was = store.cropRect
            store.beginInteraction("Crop")
            store.setCropRect(was)
            XCTAssertNil(store.problem, c.name)
            let now = store.cropRect
            store.endInteraction()
            XCTAssertEqual(Double(now.minX), Double(was.minX), accuracy: 2e-3, "\(c.name): min x")
            XCTAssertEqual(Double(now.minY), Double(was.minY), accuracy: 2e-3, "\(c.name): min y")
            XCTAssertEqual(
                Double(now.width), Double(was.width), accuracy: 2e-3, "\(c.name): width")
            XCTAssertEqual(
                Double(now.height), Double(was.height), accuracy: 2e-3, "\(c.name): height")
            // And the fields a crop drag has no business touching are untouched.
            let g = store.geometry
            XCTAssertEqual(g.angle, c.crop.angle, "\(c.name): angle")
            XCTAssertEqual(g.turns, c.crop.turns, "\(c.name): turns")
            XCTAssertEqual(g.flipH, c.crop.flipH, "\(c.name): flip h")
            XCTAssertEqual(g.flipV, c.crop.flipV, "\(c.name): flip v")
        }
    }

    /// Closing the tool does not edit the photograph. It is a property of the
    /// viewer, so there must be nothing to undo.
    @MainActor
    func testOpeningTheToolIsNotAnEdit() throws {
        let store = try Self.opened(Self.base(angle: 12), cropping: true)
        let held = store.geometry
        store.setCropping(false)
        XCTAssertEqual(store.geometry, held, "opening the crop tool changed the document")
    }

    /// The ratio hint a locked drag holds. It is the shape of the rectangle the
    /// engine handed back, because the engine has already applied the lock to
    /// it — not a second reading of the lock on this side.
    func testTheRatioHintIsTheShapeOfTheRectangleThatCameBack() {
        let wide = CGRect(x: 0.1, y: 0.2, width: 0.6, height: 0.3)
        XCTAssertEqual(
            try XCTUnwrap(CropOverlay.screenRatio(of: .ratio(w: 1, h: 1), showing: wide)),
            2, accuracy: 1e-9,
            "a square lock on a frame twice as wide as it is tall is 2:1 in the frame's uv")
        XCTAssertEqual(
            try XCTUnwrap(CropOverlay.screenRatio(of: .original, showing: wide)),
            2, accuracy: 1e-9, "Original is a lock like any other once the engine has applied it")
        // Free holds nothing, so a drag is free to reshape the rectangle.
        XCTAssertNil(CropOverlay.screenRatio(of: .free, showing: wide))
        // And a rectangle with no area gives no hint rather than an infinity.
        XCTAssertNil(
            CropOverlay.screenRatio(
                of: .ratio(w: 1, h: 1), showing: CGRect(x: 0, y: 0, width: 0.5, height: 0)))
    }

    // ---- the drag ---------------------------------------------------------

    func testACornerIsGrabbedBeforeTheEdgesThatMeetThere() {
        let rect = CGRect(x: 100, y: 100, width: 200, height: 120)
        XCTAssertEqual(
            CropGrip.at(CGPoint(x: 102, y: 102), in: rect),
            CropGrip.edge(left: true, right: false, top: true, bottom: false))
        // The middle of the top edge is the edge and nothing else.
        XCTAssertEqual(
            CropGrip.at(CGPoint(x: 200, y: 102), in: rect),
            CropGrip.edge(left: false, right: false, top: true, bottom: false))
        // Well inside is the region itself.
        XCTAssertEqual(CropGrip.at(CGPoint(x: 200, y: 160), in: rect), CropGrip.move)
        // And well outside is nothing at all, so a drag there is not a crop.
        XCTAssertNil(CropGrip.at(CGPoint(x: 40, y: 160), in: rect))
    }

    func testAnEdgeDragMovesOnlyThatEdge() {
        let rect = CGRect(x: 0.2, y: 0.2, width: 0.6, height: 0.6)
        let moved = CropGrip.dragged(
            rect, grip: .edge(left: true, right: false, top: false, bottom: false),
            by: CGSize(width: 0.1, height: 0.1))
        XCTAssertEqual(Double(moved.minX), 0.3, accuracy: 1e-9, "the left edge did not move")
        XCTAssertEqual(Double(moved.maxX), 0.8, accuracy: 1e-9, "the right edge moved with it")
        XCTAssertEqual(Double(moved.minY), 0.2, accuracy: 1e-9, "a vertical delta moved a side")
        XCTAssertEqual(Double(moved.maxY), 0.8, accuracy: 1e-9)
    }

    func testTheRegionDragKeepsItsSize() {
        let rect = CGRect(x: 0.2, y: 0.2, width: 0.6, height: 0.5)
        let moved = CropGrip.dragged(
            rect, grip: .move, by: CGSize(width: -0.1, height: 0.05))
        XCTAssertEqual(Double(moved.minX), 0.1, accuracy: 1e-9)
        XCTAssertEqual(Double(moved.minY), 0.25, accuracy: 1e-9)
        XCTAssertEqual(Double(moved.width), 0.6, accuracy: 1e-9)
        XCTAssertEqual(Double(moved.height), 0.5, accuracy: 1e-9)
    }

    func testAnEdgeCannotBeDraggedThroughTheOneOppositeIt() {
        let rect = CGRect(x: 0.2, y: 0.2, width: 0.6, height: 0.6)
        let moved = CropGrip.dragged(
            rect, grip: .edge(left: true, right: false, top: false, bottom: false),
            by: CGSize(width: 5, height: 0))
        XCTAssertEqual(
            Double(moved.width), Double(CropGrip.minimumSize), accuracy: 1e-9,
            "the crop turned inside out")
    }

    /// Zoomed in, the same rectangle covers more of the screen. The two used to
    /// be assumed identical on the Windows side, which is why its viewer was
    /// pinned to fit whenever the crop tool was open.
    func testTheRectangleFollowsTheViewerIntoAZoom() {
        let uv = CGRect(x: 0.25, y: 0.25, width: 0.5, height: 0.5)
        let target = CGRect(x: 0, y: 0, width: 400, height: 400)
        let fitted = CropOverlay.place(
            uv, in: target, showing: CGRect(x: 0, y: 0, width: 1, height: 1))
        XCTAssertEqual(fitted, CGRect(x: 100, y: 100, width: 200, height: 200))

        // Twice in, looking at the middle: the same crop is twice the size and
        // still centred.
        let zoomed = CropOverlay.place(
            uv, in: target, showing: CGRect(x: 0.25, y: 0.25, width: 0.5, height: 0.5))
        XCTAssertEqual(zoomed, CGRect(x: 0, y: 0, width: 400, height: 400))
    }

    // ---- the engine has the last word -------------------------------------

    /// The test this whole design exists for.
    ///
    /// A corner dragged well past the edge of the photograph is a crop the
    /// renderer cannot produce. The engine slides it back inside and hands back
    /// what it stored; the overlay draws *that*. Drawing the proposal instead
    /// puts a rectangle over the blank space beyond the picture, which then
    /// jumps the instant the drag ends.
    @MainActor
    func testDraggingACornerPastTheFrameLeavesTheRectangleInsideIt() throws {
        let store = try Self.opened(
            GeometryValue(
                centre: .zero, size: CGSize(width: 0.5, height: 0.5), angle: 0, turns: 0,
                flipH: false, flipV: false, aspect: .free),
            cropping: true)

        let asked = CropGrip.dragged(
            store.cropRect, grip: .edge(left: true, right: false, top: true, bottom: false),
            by: CGSize(width: -0.4, height: -0.4))
        // The proposal really is off the picture, or this proves nothing.
        XCTAssertLessThan(Double(asked.minX), 0)
        XCTAssertLessThan(Double(asked.minY), 0)

        // Read mid-drag, which is when the overlay reads it: the snapshot is
        // deliberately behind until the gesture ends, so this is the engine's
        // corrected answer or it is nothing.
        store.beginInteraction("Crop")
        store.setCropRect(asked)
        XCTAssertNil(store.problem)
        let drawn = store.cropRect
        // A thousandth rather than nothing: `Geometry::fits` carries a
        // ten-thousandth of the frame as slop and a crop is measured in whole
        // pixels, so the rectangle lands on the edge rather than exactly at it.
        // The proposal was a hundred and fifty thousandths outside.
        let slop = 1e-3
        XCTAssertGreaterThanOrEqual(Double(drawn.minX), -slop, "the crop hangs off the left")
        XCTAssertGreaterThanOrEqual(Double(drawn.minY), -slop, "the crop hangs off the top")
        XCTAssertLessThanOrEqual(Double(drawn.maxX), 1 + slop, "the crop hangs off the right")
        XCTAssertLessThanOrEqual(Double(drawn.maxY), 1 + slop, "the crop hangs off the bottom")

        // Slid, not shrunk: moving a rectangle does not make it stop fitting,
        // and shrinking here would let the drag quietly change the zoom.
        XCTAssertEqual(
            Double(drawn.width), Double(asked.width), accuracy: 0.005,
            "the corner drag resized the crop instead of sliding it")
        XCTAssertEqual(Double(drawn.height), Double(asked.height), accuracy: 0.005)

        // And nothing jumps when the hand comes off: what was drawn during the
        // drag is what the document turns out to hold.
        store.endInteraction()
        let settled = store.cropRect
        XCTAssertEqual(Double(settled.minX), Double(drawn.minX), accuracy: 1e-6)
        XCTAssertEqual(Double(settled.minY), Double(drawn.minY), accuracy: 1e-6)
        XCTAssertEqual(Double(settled.width), Double(drawn.width), accuracy: 1e-6)
        XCTAssertEqual(Double(settled.height), Double(drawn.height), accuracy: 1e-6)
    }

    /// The same, straightened, where the frame the tool shows is bigger than
    /// the photograph in it. A crop that fills the enclosing frame at twenty
    /// degrees is mostly blank corner, so the engine shrinks it — and the
    /// rectangle drawn has to be the shrunk one.
    @MainActor
    func testAStraightenedCropIsBroughtInsideThePhotographAndNotJustTheFrame() throws {
        let store = try Self.opened(
            GeometryValue(
                centre: .zero, size: CGSize(width: 0.6, height: 0.6), angle: 20, turns: 0,
                flipH: false, flipV: false, aspect: .free),
            cropping: true)

        // The whole frame, which at twenty degrees includes four blank corners.
        let asked = CGRect(x: 0, y: 0, width: 1, height: 1)
        store.beginInteraction("Crop")
        store.setCropRect(asked)
        XCTAssertNil(store.problem)
        let drawn = store.cropRect
        defer { store.endInteraction() }
        XCTAssertGreaterThan(
            Double(drawn.minX), 0,
            "the rectangle reaches the edge of the straightened frame, which is blank there")
        XCTAssertGreaterThan(Double(drawn.minY), 0)
        XCTAssertLessThan(Double(drawn.maxX), 1)
        XCTAssertLessThan(Double(drawn.maxY), 1)
        XCTAssertLessThan(
            Double(drawn.width), Double(asked.width),
            "nothing was corrected, so the proposal is being drawn")
    }

    /// A locked aspect is the engine's to hold, and what comes back is what is
    /// drawn — so a square lock has to come back square *on screen*, which for
    /// a frame that is not itself square is not the same as a square in uv.
    @MainActor
    func testASquareLockComesBackSquareOnScreen() throws {
        let store = try Self.opened(
            GeometryValue(
                centre: .zero, size: CGSize(width: 0.8, height: 0.4), angle: 0, turns: 0,
                flipH: false, flipV: false, aspect: .ratio(w: 1, h: 1)),
            cropping: true)

        let r = store.cropRect
        // The frame is the whole 1024x768 source, so a square crop is 4:3 of it.
        let onScreen = (Double(r.width) * 1024) / (Double(r.height) * 768)
        XCTAssertEqual(onScreen, 1, accuracy: 0.01, "a square lock drew \(onScreen):1")
        // And the hint a locked drag holds is that same rectangle's shape, so
        // the corner stays under the pointer rather than the engine and the
        // overlay disagreeing about what square means.
        XCTAssertEqual(
            try XCTUnwrap(CropOverlay.screenRatio(of: store.geometry.aspect, showing: r)),
            Double(r.width / r.height), accuracy: 1e-9)
    }

    // ---- what it draws ----------------------------------------------------

    private static let canvas = CGSize(width: 320, height: 200)
    private static let drawn = CGRect(x: 60, y: 40, width: 200, height: 120)

    /// The dimming, which is the whole reason the tool shows the enclosing
    /// frame: everything outside the crop is still on screen, and has to read
    /// as outside.
    @MainActor
    func testOutsideTheCropIsDimmerThanInside() throws {
        let image = try Self.render(
            CropOverlayCanvas(rect: Self.drawn, showsThirds: false), over: .white)
        // Well inside, away from the rectangle's own stroke and its grips.
        let inside = Self.grey(image, x: 160, y: 100)
        // Well outside, in the band above.
        let above = Self.grey(image, x: 160, y: 15)
        let left = Self.grey(image, x: 20, y: 100)

        XCTAssertGreaterThan(inside, 200, "the crop itself was dimmed, or nothing rendered")
        XCTAssertLessThan(
            above, Int(Double(inside) * 0.8),
            "the band above the crop is not dimmed — it read \(above) against \(inside) inside")
        XCTAssertLessThan(
            left, Int(Double(inside) * 0.8),
            "the band left of the crop is not dimmed — it read \(left) against \(inside) inside")
    }

    /// A corner grip sits on a corner.
    ///
    /// Measured as weight of ink rather than as a count of lit pixels, because
    /// an antialiased one-and-a-half point line lights two rows and a three
    /// point line lights three or four — the number that separates them
    /// reliably is how much white there is, not how many pixels have some.
    @MainActor
    func testACornerGripSitsOnACorner() throws {
        let image = try Self.render(
            CropOverlayCanvas(rect: Self.drawn, showsThirds: false), over: .black)
        let r = Self.drawn
        // A window straddling the top edge, six points in from the left corner:
        // inside the bracket's arm, which reaches eighteen.
        let atCorner = Self.weight(image, x: Int(r.minX) + 6, rows: Int(r.minY) - 3...Int(r.minY) + 6)
        // And a column on the same edge that is neither corner nor the bar in
        // the middle of the edge.
        let alongTheEdge = Self.weight(image, x: 110, rows: Int(r.minY) - 3...Int(r.minY) + 6)

        XCTAssertGreaterThan(alongTheEdge, 0, "the rectangle drew no edge at all")
        XCTAssertGreaterThan(
            atCorner, Int(Double(alongTheEdge) * 1.5),
            "the corner is no heavier than the edge beside it (\(atCorner) against "
                + "\(alongTheEdge)) — there is no bracket on that corner")

        // All four of them, so a bracket drawn once and mirrored wrongly fails.
        for x in [Int(r.minX) + 6, Int(r.maxX) - 6] {
            for rows in [Int(r.minY) - 3...Int(r.minY) + 6, Int(r.maxY) - 6...Int(r.maxY) + 3] {
                XCTAssertGreaterThan(
                    Self.weight(image, x: x, rows: rows),
                    Int(Double(alongTheEdge) * 1.5),
                    "no bracket at the corner near (\(x), \(rows.lowerBound))")
            }
        }
    }

    /// The grid is a drag's, not a resting state's: a thirds grid left over the
    /// picture is a grid nobody asked for on every frame they are looking at it.
    @MainActor
    func testTheThirdsGridIsOnlyDrawnWhileDragging() throws {
        let r = Self.drawn
        // A third of the way across, well away from the edges and the grips.
        let x = Int(r.minX + r.width / 3)
        let rows = 90...110

        let resting = try Self.render(
            CropOverlayCanvas(rect: r, showsThirds: false), over: .black)
        let dragging = try Self.render(
            CropOverlayCanvas(rect: r, showsThirds: true), over: .black)

        XCTAssertEqual(
            Self.weight(resting, x: x, rows: rows), 0,
            "something is drawn a third of the way across a crop nobody is dragging")
        XCTAssertGreaterThan(
            Self.weight(dragging, x: x, rows: rows), 0,
            "no thirds grid while dragging")
    }

    // ---- reading the render ------------------------------------------------

    @MainActor
    private static func render<V: View>(_ view: V, over background: Color) throws -> CGImage {
        let renderer = ImageRenderer(
            content: view
                .frame(width: canvas.width, height: canvas.height)
                .background(background)
                .environment(\.colorScheme, .dark))
        renderer.scale = 1
        return try XCTUnwrap(renderer.cgImage, "the renderer produced no image")
    }

    private static func bytes(_ image: CGImage) -> [UInt8] {
        let (w, h) = (image.width, image.height)
        var out = [UInt8](repeating: 0, count: w * h * 4)
        if let context = CGContext(
            data: &out, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        {
            context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        }
        return out
    }

    private static func grey(_ image: CGImage, x: Int, y: Int) -> Int {
        let data = bytes(image)
        guard x >= 0, y >= 0, x < image.width, y < image.height else { return 0 }
        let i = (y * image.width + x) * 4
        return (Int(data[i]) + Int(data[i + 1]) + Int(data[i + 2])) / 3
    }

    /// How much light there is in one column over a range of rows.
    private static func weight(_ image: CGImage, x: Int, rows: ClosedRange<Int>) -> Int {
        let data = bytes(image)
        guard x >= 0, x < image.width else { return 0 }
        var total = 0
        for y in rows where y >= 0 && y < image.height {
            let i = (y * image.width + x) * 4
            total += (Int(data[i]) + Int(data[i + 1]) + Int(data[i + 2])) / 3
        }
        return total
    }
}
