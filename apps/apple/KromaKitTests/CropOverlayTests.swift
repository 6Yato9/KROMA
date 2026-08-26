import CoreGraphics
import SwiftUI
import XCTest

// Same module as the code under test; see EngineTests.swift.

/// The crop overlay: where the rectangle sits, what a drag does to it, and what
/// the thing actually looks like.
///
/// Two halves, and they fail for different reasons.
///
/// The first half pins ``CropFrame`` against `pe_core::geometry`. That mapping
/// is a second copy of `Geometry::enclosing` and `Geometry::crop_uv_in`, which
/// the plan for this tool set out to avoid and which the C ABI leaves no way to
/// avoid: `pe_session_set_geometry` carries nine scalars in and seven out, and
/// none of them is "where is the crop in the frame you are showing me". So the
/// copy exists, and the numbers below were taken from `pe_core`'s own
/// `crop_uv_in` — a quarter-turn and a flip put a rectangle somewhere entirely
/// plausible and entirely wrong, and nothing but the engine's own answer can
/// say which.
///
/// The second half renders the overlay headlessly and reads the bitmap back,
/// the way `RowMetricsTests`, `CurveBackdropTests` and `WarperCloudTests` do.
/// What that can check is where the ink lands and how heavy it is. Whether the
/// result is *pleasant* — and whether the rectangle lands on the photograph
/// when the engine is drawing it into a Metal layer underneath — stays
/// unverified until somebody looks at it.
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

    private static func model(_ g: GeometryValue) -> CropFrame {
        CropFrame(crop: g, source: source)
    }

    // ---- the mapping, against the engine's own arithmetic -----------------

    /// The numbers `pe_core::Geometry::crop_uv_in` produces for the frame
    /// `Geometry::enclosing` gives, for the same six geometries.
    ///
    /// Taken from the engine by running `crop_uv_in` over each case and
    /// printing the result to six places; the derivation is in `CropFrame.rect`.
    /// If this ever fails, the Swift copy has drifted and the overlay is
    /// drawing a rectangle somewhere the renderer does not put the crop.
    private struct Case {
        let name: String
        let crop: GeometryValue
        /// The enclosing frame's size, as a fraction of the source.
        let frame: CGSize
        /// min x, min y, max x, max y in the frame's uv.
        let rect: [Double]
    }

    private static var cases: [Case] {
        [
            Case(
                name: "plain", crop: base(),
                frame: CGSize(width: 1, height: 1),
                rect: [0.350000, 0.250130, 0.850000, 0.649870]),
            Case(
                name: "angle 12", crop: base(angle: 12),
                frame: CGSize(width: 1.134081, height: 1.255363),
                rect: [0.367700, 0.300934, 0.808699, 0.619398]),
            Case(
                name: "one quarter-turn", crop: base(turns: 1),
                frame: CGSize(width: 1, height: 1),
                rect: [0.350130, 0.350000, 0.749870, 0.850000]),
            Case(
                name: "one quarter-turn, flipped horizontally",
                crop: base(turns: 1, flipH: true),
                frame: CGSize(width: 1, height: 1),
                rect: [0.250130, 0.350000, 0.649870, 0.850000]),
            Case(
                name: "three quarter-turns, flipped vertically, 7.5 degrees",
                crop: base(angle: 7.5, turns: 3, flipV: true),
                frame: CGSize(width: 1.089339, height: 1.165480),
                rect: [0.285587, 0.362242, 0.628603, 0.821435]),
            Case(
                name: "two quarter-turns, both flips, -20 degrees",
                crop: base(angle: -20, turns: 2, flipH: true, flipV: true),
                frame: CGSize(width: 1.196208, height: 1.395720),
                rect: [0.374612, 0.320989, 0.792571, 0.607369]),
        ]
    }

    func testTheEnclosingFrameIsTheEnginesEnclosingFrame() {
        for c in Self.cases {
            let f = CropFrame.enclosing(c.crop, source: Self.source)
            XCTAssertEqual(f.size.width, c.frame.width, accuracy: 2e-5, c.name)
            XCTAssertEqual(f.size.height, c.frame.height, accuracy: 2e-5, c.name)
            // Centred, unlocked, and carrying the crop's own angle, turn and
            // flips — which is what makes the crop axis-aligned inside it.
            XCTAssertEqual(f.centre, .zero, c.name)
            XCTAssertEqual(f.aspect, .free, c.name)
            XCTAssertEqual(f.angle, c.crop.angle, c.name)
            XCTAssertEqual(f.turns, c.crop.turns, c.name)
            XCTAssertEqual(f.flipH, c.crop.flipH, c.name)
            XCTAssertEqual(f.flipV, c.crop.flipV, c.name)
        }
    }

    func testTheRectangleIsWhereTheEngineSaysTheCropIs() {
        for c in Self.cases {
            let r = Self.model(c.crop).rect
            XCTAssertEqual(Double(r.minX), c.rect[0], accuracy: 2e-5, "\(c.name): min x")
            XCTAssertEqual(Double(r.minY), c.rect[1], accuracy: 2e-5, "\(c.name): min y")
            XCTAssertEqual(Double(r.maxX), c.rect[2], accuracy: 2e-5, "\(c.name): max x")
            XCTAssertEqual(Double(r.maxY), c.rect[3], accuracy: 2e-5, "\(c.name): max y")
        }
    }

    /// Reading the rectangle and writing it back is the round trip every drag
    /// makes, so a mismatch here moves the crop a little on every frame of a
    /// gesture that was not supposed to move it at all.
    ///
    /// The height comes back as 307/768 rather than 0.4 because the engine
    /// measures a crop in whole pixels — `output_size` rounds — and this side
    /// rounds with it rather than around it.
    func testPuttingTheRectangleBackWhereItIsChangesNothing() {
        for c in Self.cases {
            let model = Self.model(c.crop)
            let same = model.proposing(model.rect)
            XCTAssertEqual(Double(same.centre.x), 0.1, accuracy: 1e-6, "\(c.name): centre x")
            XCTAssertEqual(Double(same.centre.y), -0.05, accuracy: 1e-6, "\(c.name): centre y")
            XCTAssertEqual(Double(same.size.width), 0.5, accuracy: 1e-6, "\(c.name): width")
            XCTAssertEqual(
                Double(same.size.height), 307.0 / 768.0, accuracy: 1e-6, "\(c.name): height")
            // And the fields a drag has no business touching are untouched.
            XCTAssertEqual(same.angle, c.crop.angle, "\(c.name): angle")
            XCTAssertEqual(same.turns, c.crop.turns, "\(c.name): turns")
            XCTAssertEqual(same.flipH, c.crop.flipH, "\(c.name): flip h")
            XCTAssertEqual(same.flipV, c.crop.flipV, "\(c.name): flip v")
        }
    }

    /// An uncropped photograph fills its frame, which is the case the overlay
    /// opens on and the one where a sign error is invisible everywhere else.
    func testAnUncroppedPhotographFillsTheFrame() {
        let r = Self.model(.identity).rect
        XCTAssertEqual(Double(r.minX), 0, accuracy: 1e-6)
        XCTAssertEqual(Double(r.minY), 0, accuracy: 1e-6)
        XCTAssertEqual(Double(r.width), 1, accuracy: 1e-6)
        XCTAssertEqual(Double(r.height), 1, accuracy: 1e-6)
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
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 1024, height: 768)
        store.setGeometry(
            GeometryValue(
                centre: .zero, size: CGSize(width: 0.5, height: 0.5), angle: 0, turns: 0,
                flipH: false, flipV: false, aspect: .free))
        XCTAssertNil(store.problem)

        let source = CGSize(width: 1024, height: 768)
        let model = CropFrame(crop: store.geometry, source: source)
        let asked = CropGrip.dragged(
            model.rect, grip: .edge(left: true, right: false, top: true, bottom: false),
            by: CGSize(width: -0.4, height: -0.4))
        // The proposal really is off the picture, or this proves nothing.
        XCTAssertLessThan(Double(asked.minX), 0)
        XCTAssertLessThan(Double(asked.minY), 0)

        // Read mid-drag, which is when the overlay reads it: the snapshot is
        // deliberately behind until the gesture ends, so this is the engine's
        // corrected answer or it is nothing.
        store.beginInteraction("Crop")
        store.setGeometry(model.proposing(asked))
        XCTAssertNil(store.problem)
        let drawn = CropFrame(crop: store.geometry, source: source).rect
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
        let settled = CropFrame(crop: store.geometry, source: source).rect
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
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 1024, height: 768)
        store.setGeometry(
            GeometryValue(
                centre: .zero, size: CGSize(width: 0.6, height: 0.6), angle: 20, turns: 0,
                flipH: false, flipV: false, aspect: .free))
        XCTAssertNil(store.problem)

        let source = CGSize(width: 1024, height: 768)
        let model = CropFrame(crop: store.geometry, source: source)
        // The whole frame, which at twenty degrees includes four blank corners.
        let asked = CGRect(x: 0, y: 0, width: 1, height: 1)
        store.beginInteraction("Crop")
        store.setGeometry(model.proposing(asked))
        XCTAssertNil(store.problem)
        let drawn = CropFrame(crop: store.geometry, source: source).rect
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
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 1024, height: 768)
        store.setGeometry(
            GeometryValue(
                centre: .zero, size: CGSize(width: 0.8, height: 0.4), angle: 0, turns: 0,
                flipH: false, flipV: false, aspect: .ratio(w: 1, h: 1)))
        XCTAssertNil(store.problem)

        let source = CGSize(width: 1024, height: 768)
        let model = CropFrame(crop: store.geometry, source: source)
        let r = model.rect
        // The frame is the whole 1024x768 source, so a square crop is 4:3 of it.
        let onScreen = (Double(r.width) * 1024) / (Double(r.height) * 768)
        XCTAssertEqual(onScreen, 1, accuracy: 0.01, "a square lock drew \(onScreen):1")
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
