import CoreGraphics
import SwiftUI
import XCTest

/// Where the photograph's own colours get drawn on each of the warper's three
/// plots.
///
/// This machine cannot look at the screen, so everything here either exercises
/// the arithmetic directly or builds the image and reads the pixels back — the
/// way `ScopeViewsTests` and `CurveBackdropTests` do. What that catches is a
/// cloud in the wrong *place*, which is the whole point: a mirrored or
/// quarter-turned cloud looks entirely plausible and is confidently wrong.
final class WarperCloudTests: XCTestCase {

    /// The engine's grid resolution, `pe_scopes::warper::GRID`.
    private static let grid = 128

    // ---- the mapping, which is where this goes wrong if anywhere ----------

    /// The plan's first claim, stated the way the drawing reads it: the middle
    /// of the plot reads the middle of the square, which is where a colour with
    /// no saturation at all is binned.
    func testTheSquaresCentreIsTheDiscsCentre() throws {
        let g = try XCTUnwrap(WarperCloud.gridFraction(.hueSat, u: 0.5, v: 0.5))
        XCTAssertEqual(g.u, 0.5, accuracy: 1e-12)
        XCTAssertEqual(g.v, 0.5, accuracy: 1e-12)
    }

    /// And the second: the square's mid-right edge is full saturation on the
    /// red axis.
    ///
    /// Read from the plot's side, that is — the point at full saturation along
    /// the red axis reads the square's mid-right edge. `WarpGeometry` puts full
    /// saturation `radiusFraction` of the plot's width from the middle, and hue
    /// zero to the right, so that point is `(0.5 + radiusFraction, 0.5)`.
    func testTheSquaresMidRightEdgeIsFullSaturationOnTheRedAxis() throws {
        let r = Double(WarpGeometry.radiusFraction)
        let g = try XCTUnwrap(WarperCloud.gridFraction(.hueSat, u: 0.5 + r, v: 0.5))
        XCTAssertEqual(g.u, 1.0, accuracy: 1e-12, "full saturation is the square's edge")
        XCTAssertEqual(g.v, 0.5, accuracy: 1e-12, "the red axis is the square's middle row")

        // And the three other cardinal hues, which is what says the disc is not
        // a quarter turn out. Hue runs anticlockwise with v measured upwards,
        // the way `WarpGeometry.toScreen` draws it and the way the engine bins
        // it: `(sat·cos h, sat·sin h)`.
        let up = try XCTUnwrap(WarperCloud.gridFraction(.hueSat, u: 0.5, v: 0.5 + r))
        XCTAssertEqual(up.u, 0.5, accuracy: 1e-12)
        XCTAssertEqual(up.v, 1.0, accuracy: 1e-12, "a quarter turn anticlockwise is +y")
        let left = try XCTUnwrap(WarperCloud.gridFraction(.hueSat, u: 0.5 - r, v: 0.5))
        XCTAssertEqual(left.u, 0.0, accuracy: 1e-12)
        let down = try XCTUnwrap(WarperCloud.gridFraction(.hueSat, u: 0.5, v: 0.5 - r))
        XCTAssertEqual(down.v, 0.0, accuracy: 1e-12)
    }

    /// The square is *not* `WarpGeometry`'s polar mapping, and reading it as
    /// though it were is the failure this whole plan is about: the cloud lands
    /// on the right plot in the wrong place.
    ///
    /// Half way out along the red axis, the square says (0.75, 0.5) — half a
    /// unit of `cos h` past the middle. Read as polar it would say hue 0,
    /// saturation 0.5, which is (0, 0.5): a different cell of the same grid, so
    /// nothing would look broken.
    func testTheHueSatSquareIsNotThePolarMappingTheLatticeUses() throws {
        let r = Double(WarpGeometry.radiusFraction)
        let plot = CGPoint(x: 0.5 + r * 0.5, y: 0.5)
        let g = try XCTUnwrap(WarperCloud.gridFraction(.hueSat, u: plot.x, v: plot.y))
        XCTAssertEqual(g.u, 0.75, accuracy: 1e-12)
        XCTAssertEqual(g.v, 0.5, accuracy: 1e-12)

        // The same point, read the lattice's way. A square rect of side 1 puts
        // the plot fractions straight into `fromScreen`, remembering that view
        // y grows downwards.
        let polar = WarpGeometry(
            warp: WarpValue(cols: 1, rows: 1, offsets: [.zero]), axes: .hueSat,
            rect: CGRect(x: 0, y: 0, width: 1, height: 1)
        ).fromScreen(CGPoint(x: plot.x, y: 1 - plot.y))
        XCTAssertEqual(Double(polar.y), 0.5, accuracy: 1e-6, "half a radius out")
        XCTAssertNotEqual(
            g.u, Double(polar.x), accuracy: 0.2,
            "the square and the polar reading agreed, so this test proves nothing")
    }

    /// Outside the disc there are no colours, so there is nothing to say there.
    /// The blur spreads counts past the boundary happily, and drawing them
    /// would put a haze over a region that has no colours in it by definition.
    func testOutsideTheDiscThereAreNoColours() {
        let r = Double(WarpGeometry.radiusFraction)
        XCTAssertNil(WarperCloud.gridFraction(.hueSat, u: 0, v: 0), "a corner")
        XCTAssertNil(WarperCloud.gridFraction(.hueSat, u: 1, v: 1), "a corner")
        XCTAssertNil(
            WarperCloud.gridFraction(.hueSat, u: 0.5 + r * 1.02, v: 0.5),
            "just past full saturation")
        XCTAssertNotNil(
            WarperCloud.gridFraction(.hueSat, u: 0.5 + r * 0.98, v: 0.5),
            "just inside full saturation")
    }

    /// The other two grids are binned in their plots' own terms already — the
    /// chromaticity one with `pe_core::pins::plot_fraction`, which is the same
    /// mapping `PinGeometry` uses, and the chroma/luma one 0…1 on each axis. So
    /// a plot fraction *is* a grid fraction, and nothing is converted.
    func testTheOtherTwoGridsAreReadAtTheirPlotsOwnFractions() throws {
        for plot in [WarperCloud.Plot.chromaticity, .chromaLuma] {
            let g = try XCTUnwrap(WarperCloud.gridFraction(plot, u: 0.3, v: 0.7))
            XCTAssertEqual(g.u, 0.3, accuracy: 1e-12, "\(plot)")
            XCTAssertEqual(g.v, 0.7, accuracy: 1e-12, "\(plot)")
        }
    }

    /// `bump` stores v downwards — row zero is v = 1 — and every plot here
    /// reads it upwards. Getting this the wrong way round flips a cloud
    /// vertically and leaves it looking entirely plausible.
    func testTheGridStoresVDownwardsAndThePlotReadsItUpwards() {
        // Four cells, one count in the top-left.
        let grid: [Double] = [9, 0, 0, 0]
        XCTAssertEqual(
            WarperCloud.sample(grid, width: 2, height: 2, u: 0.25, v: 0.75), 9,
            "row zero is the top of the plot, which is v = 1")
        XCTAssertEqual(
            WarperCloud.sample(grid, width: 2, height: 2, u: 0.25, v: 0.25), 0,
            "the bottom of the plot read row zero, so the cloud is upside down")
    }

    // ---- the counts -------------------------------------------------------

    /// Blurred before it is drawn. At 128² most cells hold nothing or one, and
    /// reading between them bilinearly still shows the lattice, because the
    /// lattice is genuinely what the counts look like.
    ///
    /// The kernel is the outer product of `[1, 4, 6, 4, 1] / 16` with itself,
    /// so a lone count leaves 36/256 where it was and 24/256 beside it, and
    /// nothing at all three cells out.
    func testACloudIsSpreadOverItsNeighboursBeforeItIsDrawn() {
        let plane = Self.plane(count: 1, atCol: 64, row: 64)
        let blurred = WarperCloud.blurred(plane)
        let at = { (dx: Int, dy: Int) in blurred[(64 + dy) * Self.grid + 64 + dx] }
        XCTAssertEqual(at(0, 0), 36.0 / 256, accuracy: 1e-9)
        XCTAssertEqual(at(1, 0), 24.0 / 256, accuracy: 1e-9, "no spread across")
        XCTAssertEqual(at(0, 1), 24.0 / 256, accuracy: 1e-9, "no spread down")
        XCTAssertEqual(at(1, 1), 16.0 / 256, accuracy: 1e-9)
        XCTAssertEqual(at(2, 0), 6.0 / 256, accuracy: 1e-9)
        XCTAssertEqual(at(3, 0), 0, accuracy: 1e-12, "the kernel is five wide, not seven")
    }

    /// A fourth root, not a linear scale.
    ///
    /// A photograph's colours are wildly unevenly distributed: a sky is
    /// thousands of pixels in a handful of cells and a red jacket is a hundred
    /// over dozens. Linear, the jacket is a fraction of a byte — which is to
    /// say invisible — and seeing the jacket is the entire point.
    func testDensityIsAFourthRootSoAThinCloudIsStillVisible() {
        XCTAssertEqual(WarperCloud.haze(10_000, peak: 10_000), 0.85, accuracy: 1e-12)
        // A ten-thousandth of the peak: a tenth of full haze, twenty-one of the
        // two hundred and fifty-five a byte has.
        let thin = WarperCloud.haze(1, peak: 10_000)
        XCTAssertEqual(thin, 0.085, accuracy: 1e-9)
        XCTAssertGreaterThan(
            UInt8(thin * 255), 20,
            "a linear scale would draw this at nought, and the jacket is the point")
        XCTAssertEqual(WarperCloud.haze(0, peak: 10_000), 0)
        XCTAssertEqual(WarperCloud.haze(5, peak: 0), 0, "nothing measured, nothing drawn")
    }

    /// Nothing measured on a plot draws no haze at all, rather than a black
    /// square over it.
    func testAnEmptyMeasurementDrawsNoCloud() {
        let blank = Scopes.Plane(
            counts: [UInt32](repeating: 0, count: Self.grid * Self.grid),
            width: Self.grid, height: Self.grid, total: 0, peak: 0)
        XCTAssertNil(WarperCloud.image(blank, plot: .hueSat))
        XCTAssertNil(WarperCloud.image(blank, plot: .chromaticity))
        XCTAssertNil(WarperCloud.image(blank, plot: .chromaLuma))
    }

    // ---- and the same thing end to end, in pixels --------------------------

    /// A count at the square's mid-right edge draws at full saturation on the
    /// red axis — measured off the image, not off the arithmetic.
    ///
    /// Read as polar this would land at half a radius out; read edge to edge in
    /// the plot's own square it would land hard against the right-hand frame.
    func testAHueSatCountAtTheSquaresEdgeDrawsAtFullSaturation() throws {
        let plane = Self.plane(count: 500, atCol: Self.grid - 1, row: Self.grid / 2)
        let image = try XCTUnwrap(WarperCloud.image(plane, plot: .hueSat))
        let (u, v) = try Self.brightest(image)

        let r = Double(WarpGeometry.radiusFraction)
        let (x, y) = ((u - 0.5) / r, (v - 0.5) / r)
        XCTAssertEqual(
            (x * x + y * y).squareRoot(), 1.0, accuracy: 0.06,
            "the cloud peaked at (\(u), \(v)), which is not full saturation")
        XCTAssertEqual(
            atan2(y, x) * 180 / .pi, 0, accuracy: 5,
            "the cloud peaked off the red axis, at (\(u), \(v))")
    }

    /// And a count in the grid's top half draws in the plot's top half.
    ///
    /// The grid stores v downwards. Read the wrong way round this cloud would
    /// come out at v = 0.194 rather than 0.806 — still on the plot, still
    /// plausible, and the opposite hue.
    func testAHueSatCountInTheGridsTopHalfDrawsInThePlotsTopHalf() throws {
        let plane = Self.plane(count: 500, atCol: Self.grid / 2, row: 20)
        let image = try XCTUnwrap(WarperCloud.image(plane, plot: .hueSat))
        let (u, v) = try Self.brightest(image)

        let r = Double(WarpGeometry.radiusFraction)
        let want = 0.5 + (1 - (20.5 / Double(Self.grid)) * 2) * r
        XCTAssertEqual(
            v, want, accuracy: 0.02,
            "row 20 drew at v = \(v); upwards it belongs at \(want) and "
                + "downwards it would land at \(1 - want)")
        XCTAssertEqual(u, 0.5, accuracy: 0.02, "it should still be on the vertical axis")
    }

    /// The one the scopes plan was written for: the cloud and the pins have to
    /// agree about where a colour is.
    ///
    /// The engine bins the chromaticity grid with `pe_core::pins::plot_fraction`
    /// — the same mapping `PinGeometry` uses — so a pixel of sRGB red, whose xy
    /// is (0.64, 0.33), must draw under the pin `PinGeometry` places on that
    /// chromaticity. Binned over a range of its own it sat about six per cent
    /// of the plot away from it.
    func testTheChromaticityCloudLandsUnderThePinThatMarksTheSameColour() throws {
        let red = CGPoint(x: 0.64, y: 0.33)
        // Which cell the engine's `bump` puts it in, spelled the same way.
        let col = Int(PinGeometry.fraction(of: red.x) * Double(Self.grid))
        let row = (Self.grid - 1) - Int(PinGeometry.fraction(of: red.y) * Double(Self.grid))
        let plane = Self.plane(count: 500, atCol: col, row: row)

        let image = try XCTUnwrap(WarperCloud.image(plane, plot: .chromaticity))
        let (u, v) = try Self.brightest(image)

        let side = CGFloat(image.width)
        let drawn = CGPoint(x: u * Double(side), y: (1 - v) * Double(side))
        let pin = PinGeometry(pins: [], rect: CGRect(x: 0, y: 0, width: side, height: side))
            .screen(of: red)
        XCTAssertEqual(
            hypot(drawn.x - pin.x, drawn.y - pin.y), 0, accuracy: 3,
            "the cloud drew red at \(drawn) and the pin for it goes at \(pin), "
                + "on a plot \(Int(side)) across")
    }

    /// Chroma across, luma up, both edge to edge. A bright saturated colour
    /// belongs at the top right.
    func testAChromaLumaCountDrawsWhereItsOwnPlotSaysItIs() throws {
        let plane = Self.plane(count: 500, atCol: 100, row: 20)
        let image = try XCTUnwrap(WarperCloud.image(plane, plot: .chromaLuma))
        let (u, v) = try Self.brightest(image)
        XCTAssertEqual(u, 100.5 / Double(Self.grid), accuracy: 0.02, "chroma runs across")
        XCTAssertEqual(
            v, 1 - 20.5 / Double(Self.grid), accuracy: 0.02,
            "luma runs up, and row 20 is near the top")
    }

    // ---- how it is composited ---------------------------------------------

    /// Added to the space beneath, not painted over it.
    ///
    /// The haze brightens the plot's own colours where the photograph has them,
    /// so a dense cloud over an orange still reads as an orange with a lot of
    /// pixels in it. Blended instead, every channel is pulled the same way
    /// towards white and the difference between them — which is the colour —
    /// collapses.
    @MainActor
    func testTheCloudIsAddedToTheSpaceBeneathRatherThanBlendedOverIt() throws {
        let under = Color(red: 0.12, green: 0.04, blue: 0)
        let view = ZStack {
            under
            WarperCloudView(
                clouds: Self.clouds(chromaLuma: Self.uniform(count: 400)),
                plot: .chromaLuma, generation: 1)
        }
        let image = try Self.render(view, width: 40, height: 40)
        let lit = Self.pixel(image, x: 20, y: 20)
        let bare = Self.pixel(try Self.render(under, width: 40, height: 40), x: 20, y: 20)

        XCTAssertGreaterThan(
            Int(lit.r) - Int(bare.r), 100, "no haze was drawn at all: \(lit) against \(bare)")
        // Added, the gap between the channels survives; blended, it is scaled
        // by what is left of the background, which at this density is a
        // seventh of it.
        XCTAssertEqual(
            Int(lit.r) - Int(lit.g), Int(bare.r) - Int(bare.g), accuracy: 4,
            "the space beneath came out at \(lit) over \(bare): the colour has "
                + "been washed out rather than brightened")
        XCTAssertGreaterThan(
            Int(lit.r) - Int(lit.g), 8,
            "the difference between the channels is too small to have proved anything")
    }

    /// The haze goes under the lattice, not over it.
    ///
    /// A moved vertex is an opaque disc of the accent colour, so wherever one
    /// sits over the haze its pixels are exactly the vertex's own — the same
    /// pixels the editor draws with no measurement at all. An additive layer
    /// laid over the top would brighten every one of them.
    ///
    /// Where the vertices are is asked of `WarpGeometry` rather than hunted for
    /// by colour, so this does not depend on what the machine's accent colour
    /// happens to be.
    @MainActor
    func testTheCloudGoesUnderTheLatticeAndNotOverIt() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 64, height: 64)
        // Every vertex nudged, so every one of them is drawn as a solid disc
        // rather than the translucent one an untouched vertex gets. The nudge
        // is far too small to move the lattice anywhere.
        let nudged = WarpValue(
            cols: 6, rows: 6,
            offsets: Array(repeating: CGPoint(x: 0.0005, y: 0), count: 36))
        let side = 200
        let view = WarpEditor(
            param: try Self.warpParam(key: "hue_sat"), axes: .hueSat, row: 0,
            value: nudged, isActive: true, store: store)

        let bare = try Self.render(view, width: CGFloat(side), height: CGFloat(side))
        store.requestScopes(ScopeSize(width: 64, height: 48))
        XCTAssertTrue(store.measureScopesIfNeeded(), "nothing measured, so nothing to draw")
        XCTAssertGreaterThan(
            try XCTUnwrap(store.scopes).warper.hueSat.peak, 0,
            "the test chart measured no hue or saturation at all")
        let hazed = try Self.render(view, width: CGFloat(side), height: CGFloat(side))

        let a = Self.bytes(bare), b = Self.bytes(hazed)
        XCTAssertNotEqual(a, b, "measuring changed nothing — no cloud was drawn")

        let g = WarpGeometry(
            warp: nudged, axes: .hueSat,
            rect: CGRect(x: 0, y: 0, width: side, height: side))
        var checked = 0
        var changed: [String] = []
        for row in 0..<nudged.rows {
            for col in 0..<nudged.cols {
                let at = g.toScreen(g.displaced(col: col, row: row))
                let (x, y) = (Int(at.x), Int(at.y))
                guard x > 8, y > 8, x < side - 8, y < side - 8 else { continue }
                let dot = Self.pixel(a, x: x, y: y, width: side)
                // Only where the vertex is over cloud there is to be covered by.
                let around = [(-6, 0), (6, 0), (0, -6), (0, 6)].contains {
                    Self.pixel(a, x: x + $0.0, y: y + $0.1, width: side)
                        != Self.pixel(b, x: x + $0.0, y: y + $0.1, width: side)
                }
                guard around else { continue }
                checked += 1
                let now = Self.pixel(b, x: x, y: y, width: side)
                if now != dot { changed.append("(\(x), \(y)) \(dot) became \(now)") }
            }
        }
        XCTAssertGreaterThan(
            checked, 12,
            "found only \(checked) vertices sitting over cloud, so this proved nothing")
        XCTAssertEqual(
            changed.count, 0,
            "the cloud is being drawn over the lattice: \(changed.prefix(3))")
    }

    /// Which grid goes on which plot.
    ///
    /// Three grids rather than one because the three views are three different
    /// projections, and a cloud measured for one is meaningless on another. The
    /// pairing is asked for by plot so that no view can hold one and draw the
    /// other.
    func testEachPlotIsGivenTheGridMeasuredForIt() {
        let clouds = Scopes.WarperClouds(
            chromaticity: Self.plane(count: 11, atCol: 1, row: 1),
            hueSat: Self.plane(count: 22, atCol: 2, row: 2),
            chromaLuma: Self.plane(count: 33, atCol: 3, row: 3))
        XCTAssertEqual(clouds.plane(for: .chromaticity).peak, 11)
        XCTAssertEqual(clouds.plane(for: .hueSat).peak, 22)
        XCTAssertEqual(clouds.plane(for: .chromaLuma).peak, 33)
    }

    /// The hue/saturation lattice's haze stops at the disc.
    ///
    /// The corners of that plot are outside the unit circle: there are no
    /// colours there, and the blur spreads counts past the boundary happily. A
    /// cloud drawn edge to edge — which is what handing this plot one of the
    /// other two mappings would do — puts a haze over a region that has no
    /// colours in it by definition, and fills the corners of a round plot.
    @MainActor
    func testTheHueSatLatticesCloudStopsAtTheDiscRatherThanFillingTheSquare() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 64, height: 64)
        let side = 200
        let view = WarpEditor(
            param: try Self.warpParam(key: "hue_sat"), axes: .hueSat, row: 0,
            value: WarpValue(cols: 6, rows: 6, offsets: Array(repeating: .zero, count: 36)),
            isActive: true, store: store)
        let bare = try Self.render(view, width: CGFloat(side), height: CGFloat(side))
        store.requestScopes(ScopeSize(width: 64, height: 48))
        XCTAssertTrue(store.measureScopesIfNeeded())
        let hazed = try Self.render(view, width: CGFloat(side), height: CGFloat(side))

        let a = Self.bytes(bare), b = Self.bytes(hazed)
        XCTAssertNotEqual(a, b, "no cloud was drawn at all")
        for (x, y) in [(2, 2), (side - 3, 2), (2, side - 3), (side - 3, side - 3)] {
            XCTAssertEqual(
                Self.pixel(a, x: x, y: y, width: side),
                Self.pixel(b, x: x, y: y, width: side),
                "the corner (\(x), \(y)) is outside the disc and got a haze anyway")
        }
    }

    /// The pins plot gets a cloud too, and it is the one measured for *it*.
    ///
    /// The three distributions are three projections of the same frame, and
    /// they do not agree about where anything is. So the haze the editor
    /// actually draws is compared with what each of the three grids would draw
    /// on this plot: the chromaticity one has to be the nearest, and by a
    /// margin, or the plot is showing a real distribution measured somewhere
    /// else.
    @MainActor
    func testThePinsPlotDrawsTheGridMeasuredForIt() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 64, height: 64)
        let side = 200
        let view = PinsEditor(
            param: try Self.pinsParam(), row: 0, value: [], isActive: true, store: store)

        // The plot is the first thing in the stack, square, and left-aligned;
        // the buttons and the five rows go under it. Where it actually ends is
        // measured rather than assumed — a rect guessed a quarter too narrow
        // reads every position off by that much, which is this test's own
        // version of the bug it is looking for.
        let bare = try Self.render(view, width: CGFloat(side), height: 620)
        store.requestScopes(ScopeSize(width: 64, height: 48))
        XCTAssertTrue(store.measureScopesIfNeeded())
        let clouds = try XCTUnwrap(store.scopes).warper
        XCTAssertGreaterThan(clouds.chromaticity.peak, 0)
        let hazed = try Self.render(view, width: CGFloat(side), height: 620)

        let a = Self.bytes(bare), b = Self.bytes(hazed)
        let plot = Self.plotRect(a, width: side, height: 620)
        XCTAssertEqual(
            Double(plot.width), Double(side), accuracy: 1,
            "the plot came out \(plot), not the square this test reads positions in")

        let drawn = try XCTUnwrap(
            Self.differenceCentroid(a, b, imageWidth: side, in: plot),
            "measuring changed nothing in the plot — no cloud was drawn")

        var distances: [(String, Double)] = []
        for (name, plane) in [
            ("chromaticity", clouds.chromaticity),
            ("hue/saturation", clouds.hueSat),
            ("chroma/luma", clouds.chromaLuma),
        ] {
            let image = try XCTUnwrap(WarperCloud.image(plane, plot: .chromaticity))
            let c = try XCTUnwrap(Self.alphaCentroid(image))
            distances.append((name, hypot(c.u - drawn.u, c.v - drawn.v)))
        }
        let nearest = try XCTUnwrap(distances.min { $0.1 < $1.1 })
        XCTAssertEqual(
            nearest.0, "chromaticity",
            "the plot drew the \(nearest.0) cloud: \(distances)")
        let others = distances.filter { $0.0 != "chromaticity" }.map(\.1).min() ?? 0
        XCTAssertGreaterThan(
            others - nearest.1, 0.02,
            "the three grids sit too near one another on this frame to tell "
                + "apart: \(distances)")
    }

    /// No measurement, no cloud — and no measurement asked for. The editor is
    /// not what decides when to measure: that is a full extra render plus a
    /// readback, and a view that started one from its own body would do it on
    /// every layout pass.
    @MainActor
    func testAnEditorWithNoMeasurementDrawsNoCloudAndAsksForNone() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 64, height: 64)
        _ = try Self.render(
            WarpEditor(
                param: try Self.warpParam(key: "hue_sat"), axes: .hueSat, row: 0,
                value: WarpValue(cols: 6, rows: 6, offsets: Array(repeating: .zero, count: 36)),
                isActive: true, store: store),
            width: 200, height: 200)
        _ = try Self.render(
            PinsEditor(
                param: try Self.pinsParam(), row: 0, value: [], isActive: true, store: store),
            width: 200, height: 260)
        XCTAssertNil(store.scopes, "an editor drew and something measured a frame")
        XCTAssertNil(store.scopeRequest, "an editor asked for a measurement from its own body")
    }

    /// The wheel under the haze runs the same way the haze does.
    ///
    /// A cloud that agrees with the geometry and disagrees with the colours
    /// painted under it is worse than no cloud: the whole claim of rule three
    /// is that the haze brightens the plot's *own* colours where the
    /// photograph has them, and it cannot if the plot's colours are mirrored.
    ///
    /// `AngularGradient` sweeps clockwise, because view y grows downwards, and
    /// hue here runs anticlockwise. Painted in gradient order the wheel agreed
    /// at red and cyan and was the complementary hue everywhere else.
    @MainActor
    func testTheHueWheelRunsTheWayTheLatticeAndTheCloudDo() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 64, height: 64)
        let side = 200
        let value = WarpValue(cols: 6, rows: 6, offsets: Array(repeating: .zero, count: 36))
        let image = try Self.render(
            WarpEditor(
                param: try Self.warpParam(key: "hue_sat"), axes: .hueSat, row: 0,
                value: value, isActive: true, store: store),
            width: CGFloat(side), height: CGFloat(side))
        let data = Self.bytes(image)
        let g = WarpGeometry(
            warp: value, axes: .hueSat,
            rect: CGRect(x: 0, y: 0, width: side, height: side))

        for turns in [0.0, 0.25, 0.5, 0.75] {
            // Nine tenths of the way out, where the wash in the middle has
            // nearly gone and the hue is what is left.
            let at = g.toScreen(CGPoint(x: turns, y: 0.9))
            let p = Self.pixel(data, x: Int(at.x), y: Int(at.y), width: side)
            let got = Self.turns(of: p)
            let off = Swift.min(abs(got - turns), 1 - abs(got - turns))
            XCTAssertLessThan(
                off, 0.06,
                "the lattice puts hue \(turns) at \(at), where the plot is painted "
                    + "hue \(got) — \(p). Mirrored, it would be \(1 - turns).")
        }
    }

    // ---- helpers -----------------------------------------------------------

    /// A pixel's hue, in turns.
    private static func turns(of p: Pixel) -> Double {
        let (r, g, b) = (Double(p.r) / 255, Double(p.g) / 255, Double(p.b) / 255)
        let high = Swift.max(r, Swift.max(g, b)), low = Swift.min(r, Swift.min(g, b))
        let chroma = high - low
        guard chroma > 0.02 else { return 0 }
        let sixths: Double
        if high == r {
            sixths = ((g - b) / chroma).truncatingRemainder(dividingBy: 6)
        } else if high == g {
            sixths = (b - r) / chroma + 2
        } else {
            sixths = (r - g) / chroma + 4
        }
        let t = sixths / 6
        return t < 0 ? t + 1 : t
    }


    /// A grid with one cell counted, which is what lets a test say exactly
    /// where its cloud belongs.
    private static func plane(count: UInt32, atCol col: Int, row: Int) -> Scopes.Plane {
        var counts = [UInt32](repeating: 0, count: grid * grid)
        counts[row * grid + col] = count
        return Scopes.Plane(
            counts: counts, width: grid, height: grid, total: count, peak: count)
    }

    /// And a grid counted evenly everywhere, for the questions that are about
    /// compositing rather than about position.
    private static func uniform(count: UInt32) -> Scopes.Plane {
        Scopes.Plane(
            counts: [UInt32](repeating: count, count: grid * grid),
            width: grid, height: grid, total: count * UInt32(grid * grid), peak: count)
    }

    /// A set of three, with the others blank.
    private static func clouds(
        chromaticity: Scopes.Plane? = nil, hueSat: Scopes.Plane? = nil,
        chromaLuma: Scopes.Plane? = nil
    ) -> Scopes.WarperClouds {
        let blank = Scopes.Plane(
            counts: [UInt32](repeating: 0, count: grid * grid),
            width: grid, height: grid, total: 0, peak: 0)
        return Scopes.WarperClouds(
            chromaticity: chromaticity ?? blank, hueSat: hueSat ?? blank,
            chromaLuma: chromaLuma ?? blank)
    }

    /// The plot's own rectangle, found rather than assumed.
    ///
    /// `PinsEditor` fills it with black at 0.28 and nothing else in the view is
    /// that: premultiplied, its interior is a near-black at an alpha of about
    /// seventy. Where the plot sits depends on how the stack under it lays out,
    /// and a rect guessed wrong reads every position off by the difference —
    /// which is this test's own version of the bug it is looking for.
    private static func plotRect(_ image: [UInt8], width: Int, height: Int) -> CGRect {
        // The stack is centred in whatever frame it is given, so the plot's
        // top is the first row with any ink in it at all.
        var top = 0
        while top < height,
            !(0..<width).contains(where: { pixel(image, x: $0, y: top, width: width).a > 0 })
        { top += 1 }
        guard top < height else { return .zero }
        // Full width and square, which is what `aspectRatio(1, .fit)` in a
        // stack this narrow gives it. Checked rather than assumed: both of the
        // plot's own top corners have to be the fill, and the row below its
        // foot has to be clear of it.
        let fill = { (x: Int, y: Int) -> Bool in
            let p = pixel(image, x: x, y: y, width: width)
            return p.a > 55 && p.a < 90 && p.r < 40 && p.g < 40 && p.b < 40
        }
        guard fill(6, top + 6), fill(width - 7, top + 6), !fill(6, top + width + 3) else {
            return .zero
        }
        return CGRect(x: 0, y: top, width: width, height: width)
    }

    /// Where two renders differ, as a brightness-weighted centre in fractions
    /// of a given rectangle. What was added between them is the haze.
    private static func differenceCentroid(
        _ a: [UInt8], _ b: [UInt8], imageWidth: Int, in rect: CGRect
    ) -> (u: Double, v: Double)? {
        var (su, sv, total) = (0.0, 0.0, 0.0)
        for y in Int(rect.minY)..<Int(rect.maxY) {
            for x in Int(rect.minX)..<Int(rect.maxX) {
                let p = pixel(a, x: x, y: y, width: imageWidth)
                let q = pixel(b, x: x, y: y, width: imageWidth)
                let d = Double(Int(q.r) - Int(p.r) + Int(q.g) - Int(p.g) + Int(q.b) - Int(p.b))
                guard d > 0 else { continue }
                su += (Double(x) - rect.minX + 0.5) / rect.width * d
                sv += (1 - (Double(y) - rect.minY + 0.5) / rect.height) * d
                total += d
            }
        }
        guard total > 0 else { return nil }
        return (su / total, sv / total)
    }

    /// And the same centre for a cloud image, whose density is its alpha.
    private static func alphaCentroid(_ image: CGImage) -> (u: Double, v: Double)? {
        let n = image.width
        let data = bytes(image)
        var (su, sv, total) = (0.0, 0.0, 0.0)
        for y in 0..<n {
            for x in 0..<n {
                let d = Double(pixel(data, x: x, y: y, width: n).a)
                guard d > 0 else { continue }
                su += (Double(x) + 0.5) / Double(n) * d
                sv += (1 - (Double(y) + 0.5) / Double(n)) * d
                total += d
            }
        }
        guard total > 0 else { return nil }
        return (su / total, sv / total)
    }

    /// Where the haze is densest, as fractions of the plot — `u` across and
    /// `v` **up**, which is how every plot here is read.
    private static func brightest(_ image: CGImage) throws -> (u: Double, v: Double) {
        let n = image.width
        let data = bytes(image)
        var best: (x: Int, y: Int, value: UInt8)?
        for y in 0..<n {
            for x in 0..<n {
                let p = pixel(data, x: x, y: y, width: n)
                if best == nil || p.a > best!.value { best = (x, y, p.a) }
            }
        }
        let found = try XCTUnwrap(best, "the image was empty")
        XCTAssertGreaterThan(found.value, 0, "nothing was drawn anywhere")
        return (
            (Double(found.x) + 0.5) / Double(n),
            1 - (Double(found.y) + 0.5) / Double(n)
        )
    }

    private static func warpParam(key: String) throws -> Param {
        let json = #"{"key": "\#(key)", "name": "\#(key)", "kind": "warp"}"#
        return try JSONDecoder().decode(Param.self, from: Data(json.utf8))
    }

    private static func pinsParam() throws -> Param {
        let json = #"{"key": "pins", "name": "Pins", "kind": "pins"}"#
        return try JSONDecoder().decode(Param.self, from: Data(json.utf8))
    }

    @MainActor
    private static func render<V: View>(
        _ view: V, width: CGFloat, height: CGFloat
    ) throws -> CGImage {
        let renderer = ImageRenderer(content: view.frame(width: width, height: height))
        renderer.scale = 1
        return try XCTUnwrap(renderer.cgImage, "the renderer produced no image")
    }

    /// The image as premultiplied RGBA, which is what makes a translucent
    /// overlap comparable with an opaque disc.
    private static func bytes(_ image: CGImage) -> [UInt8] {
        let (w, h) = (image.width, image.height)
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        guard
            let context = CGContext(
                data: &bytes, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return bytes }
        context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        return bytes
    }

    private struct Pixel: Equatable, CustomStringConvertible {
        let r: UInt8, g: UInt8, b: UInt8, a: UInt8
        var description: String { "(\(r), \(g), \(b), \(a))" }
    }

    private static func pixel(_ bytes: [UInt8], x: Int, y: Int, width: Int) -> Pixel {
        let i = (y * width + x) * 4
        guard i + 3 < bytes.count else { return Pixel(r: 0, g: 0, b: 0, a: 0) }
        return Pixel(r: bytes[i], g: bytes[i + 1], b: bytes[i + 2], a: bytes[i + 3])
    }

    private static func pixel(_ image: CGImage, x: Int, y: Int) -> Pixel {
        pixel(bytes(image), x: x, y: y, width: image.width)
    }
}

