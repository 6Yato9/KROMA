import CoreGraphics
import SwiftUI
import XCTest

// Same module as the code under test; see EngineTests.swift.

/// The comparison: the cycle, where the chrome goes, and what it draws.
///
/// Three parts, and they fail for different reasons.
///
/// The first is the button's cycle and the fraction it carries round, checked
/// against a real engine — because the fraction is the engine's to keep and
/// `pe_session_set_compare` documents it as the one thing a caller can throw
/// away. A cycle that passed zero on the way past Off would look perfectly
/// correct until somebody moved the seam and came back to it.
///
/// The second is the arithmetic: where the seam lands for a fraction, and where
/// the two halves of a side by side sit. That has to agree with
/// `Session::composite`, which is what actually draws the pictures — a seam
/// painted anywhere but over the engine's own discontinuity says the picture
/// changes somewhere it does not.
///
/// The third renders the chrome headlessly and reads the bitmap back, the way
/// `RowMetricsTests`, `CurveBackdropTests`, `WarperCloudTests`, `FilmstripTests`
/// and `CropOverlayTests` do. What that can check is where the ink lands.
/// Whether the seam is the right *weight* over a photograph stays unverified
/// until somebody looks at it.
final class CompareOverlayTests: XCTestCase {

    // ---- the cycle --------------------------------------------------------

    func testTheModesCycleAndComeBackToOff() {
        var mode = Compare.off
        var seen: [Compare] = []
        for _ in 0..<3 {
            mode = mode.next
            seen.append(mode)
        }
        XCTAssertEqual(seen, [.wipe, .side, .off], "the cycle does not return to off")
        XCTAssertFalse(Compare.off.on)
        XCTAssertTrue(Compare.wipe.on && Compare.side.on)
    }

    /// The label says which mode it is in, as the Windows one does — the state
    /// is on the button rather than in something the reader has to remember.
    func testEachModeSaysWhichItIs() {
        XCTAssertEqual(Compare.off.label, "Compare")
        XCTAssertEqual(Compare.wipe.label, "Compare · Wipe")
        XCTAssertEqual(Compare.side.label, "Compare · Side")
        XCTAssertEqual(Set(Compare.allCases.map(\.label)).count, 3, "two modes read the same")
    }

    /// A fresh session's seam is already in the middle, so a first wipe begins
    /// there rather than hard against the left edge. The store reads it rather
    /// than starting a mirror of its own at zero.
    @MainActor
    func testAFreshStoreOpensWithTheSeamInTheMiddle() throws {
        let store = try XCTUnwrap(SessionStore())
        XCTAssertEqual(store.compare, .off, "a session opened comparing")
        XCTAssertEqual(Double(store.wipe), 0.5, accuracy: 1e-6)
    }

    /// The test the cycle exists for. Move the seam, go all the way round, and
    /// the seam is still where it was left.
    @MainActor
    func testCyclingAllTheWayRoundKeepsTheSeamWhereTheUserLeftIt() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        store.setCompare(.wipe)
        store.setWipe(0.25)
        XCTAssertEqual(store.compare, .wipe)
        XCTAssertEqual(Double(store.wipe), 0.25, accuracy: 1e-6)

        store.cycleCompare()
        XCTAssertEqual(store.compare, .side)
        XCTAssertEqual(
            Double(store.wipe), 0.25, accuracy: 1e-6,
            "side by side flattened the seam it does not draw")
        store.cycleCompare()
        XCTAssertEqual(store.compare, .off)
        XCTAssertEqual(
            Double(store.wipe), 0.25, accuracy: 1e-6,
            "turning the comparison off threw the seam away")
        store.cycleCompare()
        XCTAssertEqual(store.compare, .wipe)
        XCTAssertEqual(
            Double(store.wipe), 0.25, accuracy: 1e-6,
            "the wipe came back at \(store.wipe) rather than where it was left")
        XCTAssertNil(store.problem)
    }

    /// The engine clamps rather than refusing, because past either end is what
    /// dragging against the edge of a window produces — and what it stored is
    /// what the store mirrors, not what it was asked for.
    @MainActor
    func testASeamDraggedPastTheEdgeIsHeldAtIt() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        store.setCompare(.wipe)
        store.setWipe(-3)
        XCTAssertEqual(Double(store.wipe), 0, accuracy: 1e-6)
        store.setWipe(4)
        XCTAssertEqual(Double(store.wipe), 1, accuracy: 1e-6)
        XCTAssertNil(store.problem)
    }

    /// Comparing is a property of the window, not of the photograph: there has
    /// to be nothing to undo.
    @MainActor
    func testComparingIsNotAnEdit() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        let before = store.snapshot.version
        store.setCompare(.wipe)
        store.setWipe(0.3)
        store.cycleCompare()
        XCTAssertEqual(
            store.snapshot.version, before, "comparing changed the document")

        // And the version is a number that does move, so the assertion above is
        // not a comparison of two constants.
        XCTAssertNotNil(store.addEffect("exposure"), "no effect was added")
        XCTAssertNotEqual(
            store.snapshot.version, before,
            "the snapshot version does not move for an edit either, so nothing "
                + "above was checked")
    }

    // ---- where the chrome goes --------------------------------------------

    /// The seam sits at the fraction across the whole viewer, because that is
    /// where the engine's scissor rectangle ends.
    func testTheSeamSitsWhereTheWipeSays() {
        XCTAssertEqual(CompareGeometry.seam(wipe: 0.5, width: 400), 200, accuracy: 1e-9)
        XCTAssertEqual(CompareGeometry.seam(wipe: 0.25, width: 400), 100, accuracy: 1e-9)
        XCTAssertEqual(CompareGeometry.seam(wipe: 0, width: 400), 0, accuracy: 1e-9)
        XCTAssertEqual(CompareGeometry.seam(wipe: 1, width: 400), 400, accuracy: 1e-9)
        // Clamped at both ends, the way the fraction the engine stored is.
        XCTAssertEqual(CompareGeometry.seam(wipe: -2, width: 400), 0, accuracy: 1e-9)
        XCTAssertEqual(CompareGeometry.seam(wipe: 7, width: 400), 400, accuracy: 1e-9)
    }

    /// And backwards, which is the half a drag uses. A round trip that did not
    /// close would slide the seam a little on every frame of a gesture that was
    /// only supposed to follow the pointer.
    func testTheFractionAPointerAsksForIsTheSeamBackwards() {
        for fraction in [0.0, 0.25, 0.5, 0.8, 1.0] {
            let x = CompareGeometry.seam(wipe: CGFloat(fraction), width: 640)
            XCTAssertEqual(
                Double(CompareGeometry.fraction(ofX: x, width: 640)), fraction,
                accuracy: 1e-9, "the round trip lost \(fraction)")
        }
        // Off either end, and a viewer with no width at all.
        XCTAssertEqual(Double(CompareGeometry.fraction(ofX: -40, width: 640)), 0)
        XCTAssertEqual(Double(CompareGeometry.fraction(ofX: 900, width: 640)), 1)
        XCTAssertEqual(Double(CompareGeometry.fraction(ofX: 10, width: 0)), 0)
    }

    /// The two halves of a side by side: a real gap between them, neither
    /// stretched, both centred. The gap is the whole point of the mode — two
    /// pictures that touch fuse into one.
    func testTheTwoHalvesLeaveAGapAndAreNotStretched() {
        let size = CGSize(width: 320, height: 200)
        let (before, after) = CompareGeometry.halves(in: size, scale: 1)

        XCTAssertGreaterThan(
            after.minX - before.maxX, 0, "the two halves touch, so they read as one picture")
        XCTAssertEqual(after.minX - before.maxX, CompareGeometry.gap, accuracy: 1e-9)
        XCTAssertEqual(before.width, after.width, accuracy: 1e-9)
        XCTAssertEqual(before.height, after.height, accuracy: 1e-9)
        // Neither is stretched: each half has the viewer's own proportions.
        for (name, half) in [("before", before), ("after", after)] {
            XCTAssertEqual(
                Double(half.width / half.height), Double(size.width / size.height),
                accuracy: 1e-9, "the \(name) half is stretched")
        }
        // Centred vertically, and reaching both edges horizontally.
        XCTAssertEqual(before.midY, size.height / 2, accuracy: 1e-9)
        XCTAssertEqual(after.midY, size.height / 2, accuracy: 1e-9)
        XCTAssertEqual(before.minX, 0, accuracy: 1e-9)
        XCTAssertEqual(after.maxX, size.width, accuracy: 1e-9)
    }

    /// The gap is a number of *pixels*, so on a Retina display it is half as
    /// many points — which is what makes it the gap the engine actually left.
    func testTheGapIsTheEnginesPixelsRatherThanPoints() {
        let size = CGSize(width: 320, height: 200)
        let (before, after) = CompareGeometry.halves(in: size, scale: 2)
        XCTAssertEqual(
            after.minX - before.maxX, CompareGeometry.gap / 2, accuracy: 1e-9,
            "the gap did not follow the display's scale")
    }

    /// A very small viewer still gets two pictures rather than a gap with
    /// slivers either side — `side_rects`'s bound.
    func testAVerySmallViewerStillGetsTwoPictures() {
        let (before, after) = CompareGeometry.halves(
            in: CGSize(width: 12, height: 8), scale: 1)
        XCTAssertGreaterThan(before.width, 0)
        XCTAssertGreaterThan(after.width, 0)
        XCTAssertLessThanOrEqual(after.minX - before.maxX, 3)
    }

    // ---- what it draws ----------------------------------------------------

    private static let canvas = CGSize(width: 320, height: 200)

    /// Off draws nothing at all. A comparison nobody asked for should cost
    /// nothing, on this side as well as in the engine.
    @MainActor
    func testNothingIsDrawnWithTheComparisonOff() throws {
        let image = try Self.render(.off, wipe: 0.3)
        // Every pixel, not a sample of them. Sampling every sixteenth column
        // let a seam straight down the middle through, which is exactly the
        // mistake this is here to catch.
        XCTAssertEqual(
            Self.weight(image, xs: 0...319, rows: 0...199), 0,
            "something is drawn with the comparison off")
    }

    /// The seam is where the wipe says, and it runs the whole height.
    @MainActor
    func testTheSeamIsDrawnWhereTheWipeSays() throws {
        let image = try Self.render(.wipe, wipe: 0.3)
        // Three tenths of three hundred and twenty is ninety-six.
        let seam = 96
        let onIt = Self.weight(image, xs: seam - 1...seam, rows: 60...140)
        XCTAssertGreaterThan(onIt, 0, "no seam was drawn at all")
        for away in [40, 160, 260] {
            XCTAssertEqual(
                Self.weight(image, xs: away...away + 1, rows: 60...140), 0,
                "there is a line at x=\(away), which is not where the wipe put the seam")
        }
        // Top to bottom: half a seam is not a seam.
        XCTAssertGreaterThan(
            Self.weight(image, xs: seam - 1...seam, rows: 0...4), 0, "no seam at the top")
        XCTAssertGreaterThan(
            Self.weight(image, xs: seam - 1...seam, rows: 195...199), 0,
            "no seam at the bottom")
    }

    /// And it moves when the wipe does, rather than sitting in the middle
    /// whatever it is told.
    @MainActor
    func testTheSeamMovesWithTheWipe() throws {
        for (fraction, seam) in [(0.25, 80), (0.5, 160), (0.75, 240)] {
            let image = try Self.render(.wipe, wipe: CGFloat(fraction))
            XCTAssertGreaterThan(
                Self.weight(image, xs: seam - 1...seam, rows: 60...140), 0,
                "a wipe of \(fraction) drew no seam at x=\(seam)")
        }
    }

    /// "Before" on the left and "After" on the right, along the top of the one
    /// picture — `draw_compare`'s placement for a wipe, where the two halves
    /// have no corners of their own to sit in.
    @MainActor
    func testAWipeLabelsTheTwoTopCorners() throws {
        let image = try Self.render(.wipe, wipe: 0.5)
        let rows = 4...24
        XCTAssertGreaterThan(
            Self.weight(image, xs: 10...50, rows: rows), 0, "nothing in the top left corner")
        XCTAssertGreaterThan(
            Self.weight(image, xs: 270...310, rows: rows), 0,
            "nothing in the top right corner")
        // And nothing between them but the seam, which is one column wide.
        XCTAssertEqual(
            Self.weight(image, xs: 100...150, rows: rows), 0,
            "there is a caption in the middle of the top edge")
    }

    /// A side by side moves the captions onto the two half pictures, which is
    /// the only placement that says which is which.
    @MainActor
    func testASideBySideLabelsTheTwoHalves() throws {
        let image = try Self.render(.side, wipe: 0.5)
        let (before, after) = CompareGeometry.halves(in: Self.canvas, scale: 1)
        // Each caption sits six points inside its half's top left corner.
        let rows = Int(before.minY) + 2...Int(before.minY) + 22
        XCTAssertGreaterThan(
            Self.weight(image, xs: Int(before.minX) + 4...Int(before.minX) + 44, rows: rows),
            0, "nothing on the before half")
        XCTAssertGreaterThan(
            Self.weight(image, xs: Int(after.minX) + 4...Int(after.minX) + 44, rows: rows),
            0, "nothing on the after half")
        // Not where a wipe would have put them: the top corners of the viewer.
        XCTAssertEqual(
            Self.weight(image, xs: 4...48, rows: 2...20), 0,
            "a side by side captioned the top of the viewer, as a wipe does")
        // And no seam, which would fuse the two into one picture.
        XCTAssertEqual(
            Self.weight(image, xs: 159...161, rows: 0...199), 0,
            "a side by side drew a seam down the middle")
    }

    // ---- reading the render ------------------------------------------------

    @MainActor
    private static func render(_ mode: Compare, wipe: CGFloat) throws -> CGImage {
        let renderer = ImageRenderer(
            content: CompareOverlayCanvas(mode: mode, wipe: wipe, scale: 1)
                .frame(width: canvas.width, height: canvas.height)
                .background(Color.black)
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

    /// How much light there is in a rectangle of the render.
    ///
    /// Weight rather than a count of lit pixels, for `CropOverlayTests`'
    /// reason: an antialiased one-and-a-half point line lights two rows and a
    /// three point one lights three or four, and the number that separates them
    /// reliably is how much white there is.
    ///
    /// The plate under a caption is dark, so it is the text on it that this
    /// weighs — against a black background a caption reads as ink and a bare
    /// plate would read as nothing, which is the honest way round: a plate with
    /// no text on it is not a label.
    private static func weight(
        _ image: CGImage, xs: ClosedRange<Int>, rows: ClosedRange<Int>
    ) -> Int {
        let data = bytes(image)
        var total = 0
        for y in rows where y >= 0 && y < image.height {
            for x in xs where x >= 0 && x < image.width {
                let i = (y * image.width + x) * 4
                total += (Int(data[i]) + Int(data[i + 1]) + Int(data[i + 2])) / 3
            }
        }
        return total
    }
}
