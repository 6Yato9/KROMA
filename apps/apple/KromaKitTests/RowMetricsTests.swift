import SwiftUI
import XCTest

/// The row's own arithmetic, and whether a panel of the stated minimum width
/// can actually draw one.
///
/// This file exists because it did not, and the application shipped for weeks
/// with every control in it drawing a clipped label — "Temperature" as
/// "rature", "Exposure" as "osure". The inspector was pinned at 260 points
/// while the label, readout and reset arrow cost 206 between them before the
/// track had a single point, and a `GeometryReader` track takes the width it is
/// offered rather than negotiating down. The fixed parts were pushed outside
/// the frame and clipped at *both* ends.
///
/// Nothing caught it. Every unit test passed, because none of them asked what a
/// row looks like at the width the application actually gives it.
///
/// What these tests pin is the arithmetic and the rendering at the stated
/// minimum. They do **not** reproduce the original overflow, because the
/// minimum width on the track changed what happens below the minimum: a row too
/// narrow now truncates rather than overflowing past both edges. The guard that
/// matters is `testAPanelIsWideEnoughForTheRowItDraws` together with
/// `ContentView` reading `RowMetrics.minimumPanel` instead of carrying a number
/// of its own — the bug was two numbers disagreeing, and there is now one.
///
/// The second half of the file asks what the row *looks* like: which way the
/// fill grows, whether the handle covers the ramp it is pointing at, and
/// whether a dead row is drawn as one. Those are appearance, and appearance is
/// exactly the kind of thing that stays broken for weeks because nothing can
/// fail on it.
final class RowMetricsTests: XCTestCase {
    /// The arithmetic, stated where it can be read.
    func testAPanelIsWideEnoughForTheRowItDraws() {
        XCTAssertGreaterThanOrEqual(
            RowMetrics.minimumPanel - RowMetrics.inset * 2,
            RowMetrics.minimumRow,
            "the panel cannot fit the row it is for"
        )
    }

    /// The track's floor is a floor on usefulness, not on looks: a slider this
    /// short gives a hundredth of its range to about half a point of travel.
    func testTheTrackKeepsEnoughTravelToAim() {
        let hundredth = RowMetrics.track / 100
        XCTAssertGreaterThan(hundredth, 0.5, "a hundredth of the range is unaimable")
    }

    /// The test that would have caught it.
    ///
    /// At the panel's own minimum width a row must leave blank space to the
    /// left of its label. A clipped label runs hard to the left edge, because
    /// the overflow is centred and the front of the word is cut off — which is
    /// exactly what "rature" is.
    @MainActor
    func testALabelIsNotClippedAtTheNarrowestPanel() throws {
        let width = RowMetrics.minimumPanel - RowMetrics.inset * 2
        let image = try Self.render(Self.sampleRow, width: width)
        let firstInk = try XCTUnwrap(
            Self.firstInkedColumn(image), "the row drew nothing at all")
        XCTAssertGreaterThan(
            firstInk, 0,
            "the label runs to the left edge, which is what a clipped label does"
        )
    }

    /// And the readout must not fall off the other end.
    @MainActor
    func testTheReadoutIsNotPushedOffTheRightEdge() throws {
        let width = RowMetrics.minimumPanel - RowMetrics.inset * 2
        let image = try Self.render(Self.sampleRow, width: width)
        let lastInk = try XCTUnwrap(
            Self.lastInkedColumn(image), "the row drew nothing at all")
        XCTAssertLessThan(
            lastInk, image.width - 1,
            "something runs to the right edge, which is what a pushed-out readout does"
        )
    }

    /// Widening the panel must not move the label — it is a fixed column, and a
    /// row that re-centres as the panel grows is a row whose parts are being
    /// laid out by the overflow rather than by the metrics.
    @MainActor
    func testWideningThePanelLeavesTheLabelWhereItWas() throws {
        let narrow = try Self.render(
            Self.sampleRow, width: RowMetrics.minimumPanel - RowMetrics.inset * 2)
        let wide = try Self.render(Self.sampleRow, width: 460)
        let a = try XCTUnwrap(Self.firstInkedColumn(narrow))
        let b = try XCTUnwrap(Self.firstInkedColumn(wide))
        XCTAssertEqual(a, b, accuracy: 3, "the label moved when the panel grew")
    }

    // ---- the box is a second control, not a second slider -----------------

    func testTheBoxIsFinerThanTheTrack() {
        // The same drag, on each. The box must move the value less.
        let bounds = Bounds(min: -1, max: 1, default: 0, neutral: 0)
        let byTrack = ScalarRow.valueDraggingTrack(bounds: bounds, from: 0, by: 40, over: 200)
        let byBox = ScalarRow.valueDraggingBox(bounds: bounds, from: 0, by: 40, over: 200)
        XCTAssertGreaterThan(abs(byTrack), abs(byBox) * 2, "the box is not finer")
    }

    /// The mark goes where the parameter does nothing — but not at either end,
    /// where the track's own end is already the mark.
    func testTheNeutralMarkIsOnlyDrawnWhereItCouldBeMissed() {
        let bipolar = Bounds(min: -1, max: 1, default: 0, neutral: 0)
        XCTAssertEqual(try XCTUnwrap(ScalarRow.neutralMark(bounds: bipolar)), 0.5, accuracy: 1e-6)

        let fromZero = Bounds(min: 0, max: 4, default: 0, neutral: 0)
        XCTAssertNil(ScalarRow.neutralMark(bounds: fromZero), "the left end is already the mark")

        let atTheTop = Bounds(min: 0, max: 1, default: 1, neutral: 1)
        XCTAssertNil(ScalarRow.neutralMark(bounds: atTheTop), "the right end is already the mark")

        // Three per cent along is inside the pointer's own width at any panel
        // size, so it is a mark nobody could see as separate from the end.
        let nearlyZero = Bounds(min: 0, max: 100, default: 3, neutral: 3)
        XCTAssertNil(ScalarRow.neutralMark(bounds: nearlyZero))
        let clear = Bounds(min: 0, max: 100, default: 5, neutral: 5)
        XCTAssertNotNil(ScalarRow.neutralMark(bounds: clear))
    }

    // ---- what a render can show ------------------------------------------

    /// A bipolar control fills out of the middle, so the sign is readable
    /// without the number.
    ///
    /// Measured along the bar's own scanline: everything left of neutral must
    /// still be the bare track, and everything between neutral and the value
    /// must be brighter than it. A fill drawn from the left end lights up the
    /// left half and fails here.
    @MainActor
    func testABipolarFillGrowsFromTheMiddle() throws {
        let bounds = Bounds(min: -1, max: 1, default: 0, neutral: 0)
        let image = try Self.render(
            Self.row(value: 0.6, bounds: bounds, unit: ""), width: Self.wide)
        let raster = Self.raster(image)

        // Clear of the neutral mark at the middle and of the pointer at 0.6.
        let left = Self.greys(raster, along: 0.02...0.44)
        let right = Self.greys(raster, along: 0.56...0.72)
        let coldest = try XCTUnwrap(right.min())
        let warmest = try XCTUnwrap(left.max())
        XCTAssertGreaterThan(
            coldest, warmest + 15,
            "the fill does not grow out of the middle: left of neutral reads "
                + "\(warmest), right of it \(coldest)")
    }

    /// And a unipolar one from its own end.
    @MainActor
    func testAUnipolarFillGrowsFromTheLeft() throws {
        let bounds = Bounds(min: 0, max: 1, default: 0, neutral: 0)
        let image = try Self.render(
            Self.row(value: 0.6, bounds: bounds, unit: ""), width: Self.wide)
        let raster = Self.raster(image)

        let filled = Self.greys(raster, along: 0.02...0.52)
        let bare = Self.greys(raster, along: 0.68...0.98)
        let coldest = try XCTUnwrap(filled.min())
        let warmest = try XCTUnwrap(bare.max())
        XCTAssertGreaterThan(
            coldest, warmest + 15,
            "the fill does not start at the left end: the left reads \(coldest), "
                + "the unfilled right \(warmest)")
    }

    /// The pointer does not cover what it points at: on a hue ramp, the colour
    /// immediately under the handle's tip must still be that hue.
    ///
    /// Two measurements, and only the first tells a pointer from a disc. The
    /// handle is drawn at its narrowest where it crosses the *top* of the
    /// track and reaches its full width only below the middle, so what it is
    /// pointing at is the least covered part of it. A disc — of any diameter —
    /// is symmetric about its own centre, so the two scanlines come out equal
    /// and the widest part of it sits squarely on the colour being read.
    ///
    /// The second is the consequence: the ramp's own hue is still visible
    /// within a handle's half-width of the tip, so the colour under the
    /// pointer can be read off the pixels beside it.
    @MainActor
    func testTheHandleDoesNotHideTheGradientBeneathIt() throws {
        let bounds = Bounds(min: 0, max: 1, default: 0, neutral: 0)
        let image = try Self.render(
            Self.row(value: 0.5, bounds: bounds, ramp: .hue, unit: ""),
            width: Self.wide, scale: Self.fine)
        let raster = Self.raster(image)
        let centre = Self.handleColumn(at: 0.5)

        let top = Self.handleFillWidth(raster, row: Self.rampTopRow, near: centre)
        let bottom = Self.handleFillWidth(raster, row: Self.rampBottomRow, near: centre)
        XCTAssertGreaterThan(bottom, 0, "no handle was drawn on the ramp at all")
        XCTAssertLessThan(
            Double(top), Double(bottom) * 0.9,
            "the handle is as wide where it points as where it stands — a disc, "
                + "not a pointer (top \(top)px, bottom \(bottom)px)")

        let seen = try XCTUnwrap(
            Self.nearestRampPixel(raster, row: Self.rampTopRow, near: centre),
            "the ramp is not visible anywhere near the handle")
        XCTAssertLessThanOrEqual(
            Double(seen.distance) / Self.fine, ScalarRow.handleHalfWidth + 0.5,
            "the handle hides the ramp for \(Double(seen.distance) / Self.fine)pt "
                + "either side of the value it marks")
        let wanted = Self.hue(of: Ramp.hue.at(0.5))
        XCTAssertLessThan(
            Self.hueDistance(seen.hue, wanted), 20,
            "the colour beside the handle is \(seen.hue)°, not the \(wanted)° the "
                + "ramp has at this value")
    }

    /// A disabled row is dimmer than an enabled one, everywhere.
    ///
    /// `.disabled` alone will not do this: it adjusts SwiftUI's semantic
    /// styles, and every colour in the row is a palette colour of its own — a
    /// row that leaned on `.disabled` would come back with a grey label and a
    /// track still drawn at full strength.
    ///
    /// "Everywhere" is asked of each of the row's four columns rather than of
    /// each pixel. Dimming puts the row in an offscreen layer, and inside one
    /// the hairlines — the neutral tick, the pointer's outline — land on
    /// slightly different subpixel coverage, so a handful of individual dark
    /// pixels come back *less* dark while everything around them halves. A
    /// per-pixel assertion measures that rasterisation difference; a
    /// per-column one measures the thing the test is named after.
    @MainActor
    func testADisabledRowIsDimmedThroughout() throws {
        let bounds = Bounds(min: -1, max: 1, default: 0, neutral: 0)
        let live = Self.raster(
            try Self.render(Self.row(value: 0.6, bounds: bounds, unit: ""), width: Self.wide))
        let dead = Self.raster(
            try Self.render(
                Self.row(value: 0.6, bounds: bounds, isActive: false, unit: ""),
                width: Self.wide))

        for (name, region) in Self.rowColumns {
            let lit = Self.ink(live, in: region)
            let dulled = Self.ink(dead, in: region)
            XCTAssertGreaterThan(lit, 0, "the \(name) column drew nothing at all")
            XCTAssertLessThan(
                Double(dulled), Double(lit) * 0.6,
                "the \(name) column is barely dimmer when disabled: "
                    + "\(dulled) against \(lit)")
        }
    }

    // ---- helpers ---------------------------------------------------------

    /// Wide enough that the track is long and the regions sampled along it are
    /// well clear of one another.
    private static let wide: CGFloat = 400
    /// Rendered at four pixels to the point where sub-point geometry is the
    /// thing being measured.
    private static let fine: CGFloat = 4

    /// The topmost and bottommost scanlines of the ramp bar, in pixels of a
    /// `fine`-scaled render. The bar is `rampHeight` tall about the row's
    /// centre, and these two rows are the same distance either side of it —
    /// which is what makes a symmetric handle come out symmetric.
    private static var rampTopRow: Int {
        Int((RowMetrics.height / 2 - ScalarRow.rampHeight / 2) * fine)
    }
    private static var rampBottomRow: Int {
        Int((RowMetrics.height / 2 + ScalarRow.rampHeight / 2) * fine) - 1
    }

    /// A row with the longest label the registry actually uses, since that is
    /// the one that clips first.
    @MainActor
    private static var sampleRow: some View {
        row(
            value: 5600,
            bounds: Bounds(min: 2000, max: 11000, default: 5600, neutral: 5600))
    }

    @MainActor
    private static func row(
        value: Float, bounds: Bounds, ramp: Ramp = .plain, isActive: Bool = true,
        unit: String = "K"
    ) -> some View {
        ScalarRow(
            name: "Temperature",
            unit: unit,
            value: value,
            bounds: bounds,
            ramp: ramp,
            isActive: isActive,
            onChange: { _ in },
            onBegin: {},
            onEnd: {}
        )
    }

    @MainActor
    private static func render<V: View>(
        _ view: V, width: CGFloat, scale: CGFloat = 1
    ) throws -> CGImage {
        // Dark, because that is what the application is and because `.primary`
        // resolves to black otherwise — black ink on the black ground below
        // would read as no ink at all and the test would pass for the wrong
        // reason.
        let renderer = ImageRenderer(
            content: view
                .frame(width: width, height: RowMetrics.height)
                .background(.black)
                .environment(\.colorScheme, .dark))
        renderer.scale = scale
        return try XCTUnwrap(renderer.cgImage, "the renderer produced no image")
    }

    // ---- reading the render ----------------------------------------------

    private struct Raster {
        let bytes: [UInt8]
        let width: Int
        let height: Int
        /// How many pixels one point came out as.
        var scale: Double { Double(height) / Double(RowMetrics.height) }
    }

    private struct Pixel {
        let r: Int
        let g: Int
        let b: Int
        var grey: Int { (r + g + b) / 3 }
        var darkest: Int { Swift.min(r, Swift.min(g, b)) }
        var brightest: Int { Swift.max(r, Swift.max(g, b)) }
        /// How colourful, as the gap between the extreme channels. The palette
        /// is entirely grey; a ramp at four-fifths saturation is not.
        var chroma: Int { brightest - darkest }
    }

    private static func raster(_ image: CGImage) -> Raster {
        let (w, h) = (image.width, image.height)
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        if let context = CGContext(
            data: &bytes, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        {
            context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        }
        return Raster(bytes: bytes, width: w, height: h)
    }

    private static func pixel(_ r: Raster, _ x: Int, _ y: Int) -> Pixel {
        guard x >= 0, y >= 0, x < r.width, y < r.height else { return Pixel(r: 0, g: 0, b: 0) }
        let i = (y * r.width + x) * 4
        return Pixel(r: Int(r.bytes[i]), g: Int(r.bytes[i + 1]), b: Int(r.bytes[i + 2]))
    }

    /// Where a fraction along the track falls, in pixels of the render.
    ///
    /// Asked of the row itself rather than worked out again here — a test that
    /// re-derives the layout it is checking is a test of its own arithmetic.
    private static func column(at t: CGFloat, scale: Double = 1) -> Int {
        let track = RowMetrics.trackWidth(inRowOf: wide)
        let span = track - ScalarRow.handleWidth
        let x = RowMetrics.label + RowMetrics.gap + ScalarRow.handleHalfWidth + t * span
        return Int(Double(x) * scale)
    }

    private static func handleColumn(at t: Float) -> Int {
        column(at: CGFloat(t), scale: Double(fine))
    }

    /// The greys along the bar's own scanline, over a stretch of the track.
    private static func greys(_ r: Raster, along range: ClosedRange<CGFloat>) -> [Int] {
        let y = r.height / 2
        let from = column(at: range.lowerBound, scale: r.scale)
        let to = column(at: range.upperBound, scale: r.scale)
        return (from...to).map { pixel(r, $0, y).grey }
    }

    /// How wide the handle's *fill* is on one scanline, in pixels.
    ///
    /// The fill only — the pale grey inside the outline. Counting the outline
    /// too would add a constant point either side and flatten exactly the
    /// difference being measured. Nothing else in the picture is both this
    /// pale and this colourless: the ramp is at four-fifths saturation and the
    /// outline is nearly black.
    private static func handleFillWidth(_ r: Raster, row y: Int, near x: Int) -> Int {
        let reach = Int(ScalarRow.handleWidth * fine)
        return (Swift.max(0, x - reach)...Swift.min(r.width - 1, x + reach)).count {
            let p = pixel(r, $0, y)
            return p.darkest > 140 && p.chroma < 40
        }
    }

    /// The nearest column to the handle where the ramp is still showing.
    private static func nearestRampPixel(
        _ r: Raster, row y: Int, near x: Int
    ) -> (distance: Int, hue: Double)? {
        for d in 0...Int(ScalarRow.handleWidth * fine) {
            for candidate in [x - d, x + d] {
                let p = pixel(r, candidate, y)
                // Well past any blend of handle grey with the ramp behind it.
                if p.chroma > 100 { return (d, hue(of: p)) }
            }
        }
        return nil
    }

    private static func hue(of p: Pixel) -> Double {
        let c = Double(p.chroma)
        guard c > 0 else { return 0 }
        let (rf, gf, bf) = (Double(p.r), Double(p.g), Double(p.b))
        var h: Double
        if p.r == p.brightest {
            h = (gf - bf) / c
        } else if p.g == p.brightest {
            h = 2 + (bf - rf) / c
        } else {
            h = 4 + (rf - gf) / c
        }
        h *= 60
        return h < 0 ? h + 360 : h
    }

    private static func hue(of c: Rgb8) -> Double {
        hue(of: Pixel(r: Int(c.r), g: Int(c.g), b: Int(c.b)))
    }

    /// Hues are a circle, so 350° and 10° are twenty degrees apart.
    private static func hueDistance(_ a: Double, _ b: Double) -> Double {
        let d = abs(a - b).truncatingRemainder(dividingBy: 360)
        return Swift.min(d, 360 - d)
    }

    /// The row's four columns, as ranges of points across it. Taken from
    /// ``RowMetrics`` rather than measured off the render, so a column that
    /// moved would show up as one that stopped dimming.
    private static var rowColumns: [(String, ClosedRange<CGFloat>)] {
        let track = RowMetrics.trackWidth(inRowOf: wide)
        let afterLabel = RowMetrics.label + RowMetrics.gap
        let afterTrack = afterLabel + track + RowMetrics.gap
        let afterValue = afterTrack + RowMetrics.value + RowMetrics.gap
        return [
            ("label", 0...RowMetrics.label),
            ("track", afterLabel...(afterLabel + track)),
            ("value", afterTrack...(afterTrack + RowMetrics.value)),
            ("reset", afterValue...(afterValue + RowMetrics.reset)),
        ]
    }

    /// Everything one column of the row draws, added up.
    private static func ink(_ r: Raster, in region: ClosedRange<CGFloat>) -> Int {
        let from = Swift.max(0, Int(Double(region.lowerBound) * r.scale))
        let to = Swift.min(r.width - 1, Int(Double(region.upperBound) * r.scale))
        guard from <= to else { return 0 }
        var total = 0
        for y in 0..<r.height {
            for x in from...to { total += pixel(r, x, y).grey }
        }
        return total
    }

    private static func columns(_ image: CGImage) -> [Bool] {
        let r = raster(image)
        // A column counts as inked if anything in it is meaningfully brighter
        // than the black ground — the row is drawn on black, so the label, the
        // track and the handle all read as light.
        return (0..<r.width).map { x in
            (0..<r.height).contains { y in
                let p = pixel(r, x, y)
                return p.r > 40 || p.g > 40 || p.b > 40
            }
        }
    }

    private static func firstInkedColumn(_ image: CGImage) -> Int? {
        columns(image).firstIndex(of: true)
    }

    private static func lastInkedColumn(_ image: CGImage) -> Int? {
        columns(image).lastIndex(of: true)
    }
}
