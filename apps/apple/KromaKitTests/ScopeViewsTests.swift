import CoreGraphics
import SwiftUI
import XCTest

// Same module as the code under test; see EngineTests.swift.
final class ScopeViewsTests: XCTestCase {

    // ---- how a count becomes a brightness --------------------------------

    func testAnEmptyCellDrawsNothing() {
        XCTAssertEqual(ScopeImage.intensity(count: 0, fullScale: 240), 0)
    }

    func testAFullCellDrawsAtFullBrightness() {
        XCTAssertEqual(ScopeImage.intensity(count: 240, fullScale: 240), 1)
    }

    /// The whole reason for the curve. A gradient spreads a column over two
    /// hundred cells and a flat sky puts it all in one; drawn linearly the
    /// gradient is one two-hundredth as bright, which is to say invisible.
    ///
    /// A linear implementation reads 0.01 here, and one with the same 0.06
    /// floor reads 0.06 — both fail, which is what makes this the test worth
    /// having. The first two pass either way.
    func testOnePerCentOfFullScaleIsBrighterThanOnePerCent() {
        let faint = ScopeImage.intensity(count: 1, fullScale: 100)
        XCTAssertGreaterThan(
            faint, 0.1,
            "a cell holding one per cent of the column came out at \(faint), which is invisible")
        XCTAssertLessThan(
            faint, ScopeImage.intensity(count: 100, fullScale: 100),
            "and it should still be dimmer than a full one")
    }

    /// The brightness scale must not depend on what is in the picture, or the
    /// whole scope would shift under the user's hand as they graded. That is
    /// the reason `fullScale` is the row count and not the observed peak.
    func testTheBrightnessScaleDoesNotMoveWithTheContent() {
        XCTAssertEqual(
            ScopeImage.intensity(count: 120, fullScale: 240),
            ScopeImage.intensity(count: 60, fullScale: 120),
            accuracy: 1e-12)
    }

    // ---- the images ------------------------------------------------------

    private func measured(width: UInt32 = 32, height: UInt32 = 24) throws -> Scopes {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        try session.measureScopes(width: width, height: height)
        return try XCTUnwrap(try session.scopes())
    }

    func testAWaveformImageIsAsWideAsTheFrameAndAsTallAsTheLevels() throws {
        let scopes = try measured()
        let raster = ScopeImage.waveform(scopes.waveform, channels: [.luma])
        XCTAssertEqual(raster.width, 32)
        XCTAssertEqual(raster.height, 256)
        XCTAssertNotNil(raster.cgImage())
    }

    func testAParadeIsThreePanelsWide() throws {
        let scopes = try measured()
        let raster = ScopeImage.parade(scopes.waveform)
        XCTAssertEqual(raster.width, 32 * 3)
        XCTAssertEqual(raster.height, 256)
    }

    /// Level zero is black and belongs at the *bottom* of the plot. Getting
    /// this upside down draws a plausible waveform of a photograph nobody took,
    /// and it is invisible on a symmetric test image — so this builds the
    /// counts by hand with one lit level well away from the middle.
    func testDarkPixelsDrawAtTheBottomOfTheWaveform() {
        let counts = Self.counts(columns: 4, litLevel: 8, count: 6, rows: 6)
        let raster = ScopeImage.waveform(counts, channels: [.luma])

        // Eight rows up from the bottom, not eight down from the top.
        XCTAssertGreaterThan(
            raster.alpha(x: 0, y: 256 - 1 - 8), 0, "level 8 drew nothing at all")
        XCTAssertEqual(
            raster.alpha(x: 0, y: 8), 0,
            "the waveform is upside down — level 8 drew near the top")
    }

    /// And the same for a parade, whose three panels each have to run the same
    /// way up.
    func testAParadeRunsTheSameWayUpInEveryPanel() {
        let counts = Self.counts(columns: 4, litLevel: 8, count: 6, rows: 6)
        let raster = ScopeImage.parade(counts)
        for panel in 0..<3 {
            let x = panel * 4
            XCTAssertGreaterThan(
                raster.alpha(x: x, y: 256 - 1 - 8), 0, "panel \(panel) drew nothing")
            XCTAssertEqual(raster.alpha(x: x, y: 8), 0, "panel \(panel) is upside down")
        }
    }

    /// Where two channels overlap the result goes pale rather than one hiding
    /// the other, which is what makes an overlaid waveform readable at all.
    func testOverlappingChannelsGoPaleRatherThanHidingEachOther() {
        // A faint cell, so nothing saturates and the addition is visible in
        // every component rather than clipping to white.
        let counts = Self.counts(columns: 2, litLevel: 100, count: 1, rows: 100)
        let red = ScopeImage.waveform(counts, channels: [.red])
        let both = ScopeImage.waveform(counts, channels: [.red, .green])
        let y = 256 - 1 - 100
        let one = red.pixel(x: 0, y: y)
        let two = both.pixel(x: 0, y: y)

        XCTAssertGreaterThan(one.r, one.g, "the red channel should read as red")
        XCTAssertGreaterThan(two.g, one.g, "green did not add to red")
        // And nothing was replaced: every component came up, which is what
        // makes an overlap read as pale rather than as whichever was last.
        XCTAssertGreaterThanOrEqual(two.r, one.r)
        XCTAssertGreaterThanOrEqual(two.b, one.b)
    }

    func testAVectorscopeImageIsTheGridsOwnSize() throws {
        let scopes = try measured()
        let raster = ScopeImage.vectorscope(scopes.vectorscope)
        XCTAssertEqual(raster.width, 256)
        XCTAssertEqual(raster.height, 256)
        XCTAssertNotNil(raster.cgImage())
    }

    // ---- the graticule ---------------------------------------------------

    private func graticuleFixture() throws -> [String: Any] {
        let url = try XCTUnwrap(
            Bundle(for: Self.self).url(forResource: "scope_graticule", withExtension: "json"),
            "scope_graticule.json is not in the test bundle"
        )
        let raw = try JSONSerialization.jsonObject(with: Data(contentsOf: url))
        return try XCTUnwrap(raw as? [String: Any])
    }

    /// The Swift projection against the Rust one it is a copy of.
    ///
    /// `pe_scopes::waveform::position` has no C ABI, so the boxes are placed by
    /// a second implementation of it. This is what stops the two parting
    /// company — the fixture is written by
    /// `cargo test -p pe-session --test fixtures`.
    func testTheColourBarTargetsSitWhereTheEngineProjectsThem() throws {
        let fixture = try graticuleFixture()
        let targets = try XCTUnwrap(fixture["targets"] as? [[String: Any]])
        XCTAssertEqual(targets.count, ScopeGraticule.targets.count)

        for (expected, target) in zip(targets, ScopeGraticule.targets) {
            let name = try XCTUnwrap(expected["name"] as? String)
            XCTAssertEqual(name, target.name)

            // The published triple itself, so a typo in one of eighteen bytes
            // is caught rather than quietly moving a box.
            let rgb = try XCTUnwrap(expected["rgb"] as? [Int])
            XCTAssertEqual(rgb, [Int(target.red), Int(target.green), Int(target.blue)], name)

            let at = try XCTUnwrap(expected["position"] as? [Double])
            let ours = ScopeGraticule.position(target)
            XCTAssertEqual(ours.x, at[0], accuracy: 1e-5, "\(name) x")
            XCTAssertEqual(ours.y, at[1], accuracy: 1e-5, "\(name) y")
        }
    }

    /// And the property the boxes exist for: a box can never end up somewhere
    /// the pixels cannot reach.
    ///
    /// The fixture's `cell` is not computed from the position — it is the bin
    /// that lights up when one pixel of that colour goes through
    /// `Vectorscope::from_display`. So this compares where the box is drawn
    /// against where the pixels actually went.
    func testEveryBoxLandsOnTheBinItsOwnColourWouldLight() throws {
        let fixture = try graticuleFixture()
        let size = try XCTUnwrap(fixture["size"] as? Int)
        XCTAssertEqual(size, 256)

        let targets = try XCTUnwrap(fixture["targets"] as? [[String: Any]])
        for (expected, target) in zip(targets, ScopeGraticule.targets) {
            let cell = try XCTUnwrap(expected["cell"] as? [Int])
            let ours = ScopeGraticule.cell(ScopeGraticule.position(target), size: size)
            XCTAssertEqual([ours.x, ours.y], cell, target.name)
        }

        let skin = try XCTUnwrap(fixture["skin"] as? [String: Any])
        let skinRGB = try XCTUnwrap(skin["rgb"] as? [Int])
        XCTAssertEqual(
            skinRGB,
            [
                Int(ScopeGraticule.skin.red), Int(ScopeGraticule.skin.green),
                Int(ScopeGraticule.skin.blue),
            ])
        let skinCell = try XCTUnwrap(skin["cell"] as? [Int])
        let oursSkin = ScopeGraticule.cell(
            ScopeGraticule.position(ScopeGraticule.skin), size: size)
        XCTAssertEqual([oursSkin.x, oursSkin.y], skinCell, "skin")
    }

    /// The unit mapping runs y **downwards**, matching the grid's own order —
    /// which is the other half of "level 0 at the bottom" and just as easy to
    /// invert without noticing.
    func testTheUnitSquareRunsYDownwardsLikeTheGrid() {
        XCTAssertEqual(ScopeGraticule.unit(CGPoint(x: 0, y: 0)), CGPoint(x: 0.5, y: 0.5))
        // Cr pointing up means +y is nearer the *top*, which is a smaller row.
        XCTAssertLessThan(ScopeGraticule.unit(CGPoint(x: 0, y: 1)).y, 0.5)
        XCTAssertGreaterThan(ScopeGraticule.unit(CGPoint(x: 0, y: -1)).y, 0.5)
    }

    /// A colour outside the plot is still a colour that is there, so the cell
    /// clamps rather than running off the end of the grid.
    func testAPositionOutsideThePlotClampsIntoIt() {
        let low = ScopeGraticule.cell(CGPoint(x: -4, y: 4), size: 256)
        XCTAssertEqual(low.x, 0)
        XCTAssertEqual(low.y, 0)
        let high = ScopeGraticule.cell(CGPoint(x: 4, y: -4), size: 256)
        XCTAssertEqual(high.x, 255)
        XCTAssertEqual(high.y, 255)
    }

    // ---- the histogram ---------------------------------------------------

    func testAHistogramTraceRunsFromNothingToFull() {
        var bins = [UInt32](repeating: 0, count: 256)
        bins[128] = 500
        let trace = HistogramView.trace(bins, peak: 500)
        XCTAssertEqual(trace.count, 256)
        XCTAssertEqual(trace[0], 0, accuracy: 1e-9, "an empty bin should draw nothing")
        XCTAssertGreaterThan(trace[128], 0.5)
        XCTAssertLessThanOrEqual(trace.max() ?? 0, 1)
        // Smoothed, so a lone spike is a curve rather than a bar.
        XCTAssertGreaterThan(trace[129], 0, "the smoothing did not reach the next bin")
        XCTAssertGreaterThan(trace[128], trace[129])
    }

    func testAnEmptyHistogramDoesNotDivideByZero() {
        let trace = HistogramView.trace([UInt32](repeating: 0, count: 256), peak: 0)
        XCTAssertEqual(trace.count, 256)
        XCTAssertTrue(trace.allSatisfy { $0 == 0 })
    }

    // ---- what the panel asks for -----------------------------------------

    /// The width is quantised so that dragging the window's corner does not
    /// order a fresh render and a 1.2 MB readback at every intermediate width.
    func testTheMeasurementSizeIsQuantisedAndBounded() {
        let a = ScopesPanel.measurement(panelWidth: 300, aspect: 0.75)
        XCTAssertEqual(a.width, 256)
        XCTAssertEqual(a.height, 192)
        XCTAssertEqual(ScopesPanel.measurement(panelWidth: 340, aspect: 0.75).width, 320)

        // Neither a sliver of a panel nor a wall of one is worth measuring at.
        XCTAssertEqual(ScopesPanel.measurement(panelWidth: 10, aspect: 0.75).width, 128)
        XCTAssertEqual(ScopesPanel.measurement(panelWidth: 4000, aspect: 0.75).width, 640)

        // A tall photograph is measured tall, up to the same ceiling.
        XCTAssertEqual(ScopesPanel.measurement(panelWidth: 4000, aspect: 3).height, 640)
        // And a nonsense aspect does not produce a zero-row frame.
        XCTAssertGreaterThanOrEqual(
            ScopesPanel.measurement(panelWidth: 640, aspect: 0).height, 96)
    }

    // ---- measuring only when somebody is looking --------------------------

    @MainActor
    func testTheStoreMeasuresForAVisiblePanelAndNotForAHiddenOne() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 64, height: 64)

        // No panel on screen: no render, no readback, nothing to draw.
        XCTAssertFalse(store.measureScopesIfNeeded(), "measured with no panel on screen")
        XCTAssertNil(store.scopes)

        store.requestScopes(ScopeSize(width: 64, height: 48))
        XCTAssertTrue(store.measureScopesIfNeeded(), "a visible panel got no measurement")
        XCTAssertNotNil(store.scopes)
        XCTAssertNil(store.problem)

        // And not again while the counts still describe what is on screen —
        // otherwise every tick is a second render of the photograph.
        XCTAssertFalse(store.measureScopesIfNeeded(), "measured again for nothing")

        // An edit throws the measurement away, so the next tick takes another.
        let row = try XCTUnwrap(store.addEffect("exposure"))
        store.setFloat(row: row, key: "ev", value: 1.0)
        XCTAssertNil(store.scopes)
        XCTAssertTrue(store.measureScopesIfNeeded(), "the grade moved and nothing re-measured")

        // Closing the panel stops it again, however stale the counts are.
        store.requestScopes(nil)
        store.setFloat(row: row, key: "ev", value: 1.5)
        XCTAssertFalse(store.measureScopesIfNeeded(), "measured behind a closed panel")
        XCTAssertNil(store.scopes)
    }

    /// A panel that has been made wider wants more columns. A waveform
    /// stretched from three hundred to six hundred is a picture of the
    /// interpolation.
    @MainActor
    func testAResizedPanelIsMeasuredAgainAtItsNewWidth() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 64, height: 64)
        store.requestScopes(ScopeSize(width: 64, height: 48))
        XCTAssertTrue(store.measureScopesIfNeeded())
        XCTAssertEqual(store.scopes?.waveform.columns, 64)

        store.requestScopes(ScopeSize(width: 128, height: 96))
        XCTAssertTrue(store.measureScopesIfNeeded())
        XCTAssertEqual(store.scopes?.waveform.columns, 128)
    }

    /// Nothing open, nothing to measure — and in particular no refusal in the
    /// status bar for a panel that is only doing its job.
    @MainActor
    func testAnEmptySessionIsNotMeasured() throws {
        let store = try XCTUnwrap(SessionStore())
        store.requestScopes(ScopeSize(width: 64, height: 48))
        XCTAssertFalse(store.measureScopesIfNeeded())
        XCTAssertNil(store.problem)
    }

    // ---- that the drawing actually happens --------------------------------

    /// The graticule rendered, rather than inspected.
    ///
    /// Everything above this point checks arithmetic. This checks that the
    /// arithmetic reaches a pixel: a `Canvas` that throws or lays out to
    /// nothing would leave a scope that looks like a scope until you tried to
    /// read it, and nothing else here would notice.
    @MainActor
    func testTheVectorscopeGraticuleReachesPixels() throws {
        let image = try Self.render(VectorscopeGraticule(), side: 200)
        XCTAssertEqual(image.width, 200)
        XCTAssertGreaterThan(Self.inked(image), 0, "the graticule drew nothing at all")
    }

    @MainActor
    func testAHistogramReachesPixels() throws {
        var bins = [UInt32](repeating: 0, count: 256)
        for i in 60..<200 { bins[i] = UInt32(i) }
        let levels = Scopes.Levels(
            red: bins, green: bins, blue: bins, luma: bins, total: 10_000, peak: 199)
        let image = try Self.render(HistogramView(levels: levels), side: 200)
        XCTAssertGreaterThan(Self.inked(image), 0, "the histogram drew nothing at all")
    }

    /// The whole waveform view, through SwiftUI, with the ink where the counts
    /// put it.
    ///
    /// Everything above tests the raster; this tests that the raster reaches
    /// the screen the right way up. A view that built its picture correctly and
    /// then flipped it on the way out would pass every other test here.
    @MainActor
    func testAWaveformViewDrawsItsDarkPixelsLow() throws {
        let counts = Self.counts(columns: 32, litLevel: 8, count: 6, rows: 6)
        let scopes = Self.scopes(waveform: counts)
        let image = try Self.render(
            WaveformView(scopes: scopes, channels: [.luma]), side: 128)

        // Trace in the bottom eighth, and none in the top half.
        XCTAssertGreaterThan(
            Self.trace(image, band: 0.88...1.0), 0,
            "level 8 did not draw near the bottom")
        XCTAssertEqual(
            Self.trace(image, band: 0.0...0.5), 0,
            "the waveform is upside down — level 8 drew in the top half")
    }

    /// How many pixels of *trace* are in a horizontal band, as a fraction of
    /// the height from the top.
    ///
    /// Thresholded on alpha, which separates the two things drawn here without
    /// having to know either's colour: a lit waveform cell is opaque and the
    /// reference lines are drawn at a tenth of that or less.
    private static func trace(_ image: CGImage, band: ClosedRange<Double>) -> Int {
        let (w, h) = (image.width, image.height)
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        guard
            let context = CGContext(
                data: &bytes, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return 0 }
        context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))

        var found = 0
        let rows = Int(band.lowerBound * Double(h))..<Int(band.upperBound * Double(h))
        for y in rows where y >= 0 && y < h {
            for x in 0..<w where bytes[(y * w + x) * 4 + 3] > 64 {
                found += 1
            }
        }
        return found
    }

    @MainActor
    private static func render<V: View>(_ view: V, side: CGFloat) throws -> CGImage {
        let renderer = ImageRenderer(content: view.frame(width: side, height: side))
        renderer.scale = 1
        return try XCTUnwrap(renderer.cgImage, "the renderer produced no image")
    }

    /// How many pixels came out with any ink on them.
    private static func inked(_ image: CGImage) -> Int {
        let (w, h) = (image.width, image.height)
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        guard
            let context = CGContext(
                data: &bytes, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return 0 }
        context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        return stride(from: 3, to: bytes.count, by: 4).reduce(0) { $0 + (bytes[$1] > 0 ? 1 : 0) }
    }

    // ---- helpers ---------------------------------------------------------

    /// A `Scopes` carrying the given waveform and nothing else, for the views
    /// that only read one of the six.
    private static func scopes(waveform: Scopes.WaveformCounts) -> Scopes {
        let empty = [UInt32](repeating: 0, count: 256)
        let levels = Scopes.Levels(
            red: empty, green: empty, blue: empty, luma: empty, total: 0, peak: 0)
        let plane = Scopes.Plane(counts: [0], width: 1, height: 1, total: 0, peak: 0)
        return Scopes(
            histogram: levels,
            logHistogram: levels,
            colour: Scopes.Spread(hue: empty, saturation: empty, total: 0, peak: 0),
            waveform: waveform,
            vectorscope: plane,
            warper: Scopes.WarperClouds(
                chromaticity: plane, hueSat: plane, chromaLuma: plane),
            generation: 1)
    }

    /// Counts with one level lit in every column of every channel, so a test
    /// can say exactly where the ink belongs.
    private static func counts(
        columns: Int, litLevel: Int, count: UInt32, rows: UInt32
    ) -> Scopes.WaveformCounts {
        let levels = 256
        var plane = [UInt32](repeating: 0, count: columns * levels)
        for column in 0..<columns {
            plane[column * levels + litLevel] = count
        }
        return Scopes.WaveformCounts(
            columns: columns, levels: levels, total: rows,
            red: plane, green: plane, blue: plane, luma: plane)
    }
}
