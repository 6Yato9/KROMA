import XCTest

final class PinGeometryTests: XCTestCase {
    private func plot() -> CGRect { CGRect(x: 0, y: 0, width: 200, height: 200) }

    private func fixture() throws -> [String: Any] {
        let url = try XCTUnwrap(
            Bundle(for: Self.self).url(forResource: "pin_samples", withExtension: "json"),
            "pin_samples.json is not in the test bundle"
        )
        return try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
        )
    }

    /// The test this file exists for. A chromaticity read as a fraction lands
    /// somewhere entirely different from a chromaticity read as a
    /// chromaticity, so this is checked against the engine rather than assumed.
    func testTheFractionMappingMatchesTheEngine() throws {
        let pairs = try XCTUnwrap(fixture()["fractions"] as? [[Double]])
        for pair in pairs {
            XCTAssertEqual(
                PinGeometry.fraction(of: pair[0]), pair[1], accuracy: 0.0001,
                "chromaticity \(pair[0])"
            )
        }
    }

    func testTheValueMappingMatchesTheEngine() throws {
        let pairs = try XCTUnwrap(fixture()["values"] as? [[Double]])
        for pair in pairs {
            XCTAssertEqual(
                PinGeometry.value(at: pair[0]), pair[1], accuracy: 0.0001,
                "fraction \(pair[0])"
            )
        }
    }

    func testThePlotRangeMatchesTheEngine() throws {
        let plot = try XCTUnwrap(fixture()["plot"] as? [String: Double])
        XCTAssertEqual(PinGeometry.plotMin, try XCTUnwrap(plot["min"]), accuracy: 0.00001)
        XCTAssertEqual(PinGeometry.plotSpan, try XCTUnwrap(plot["span"]), accuracy: 0.00001)
    }

    func testTheTwoMappingsAreInverses() {
        let g = PinGeometry(pins: [], rect: plot())
        for at in [CGPoint(x: 0.1, y: 0.2), CGPoint(x: 0.3127, y: 0.329), CGPoint(x: 0.7, y: 0.8)] {
            let back = g.chromaticity(g.screen(of: at))
            XCTAssertEqual(back.x, at.x, accuracy: 0.001)
            XCTAssertEqual(back.y, at.y, accuracy: 0.001)
        }
    }

    /// y up, which is how a chromaticity diagram is always drawn.
    func testThePlotPutsLowYAtTheBottom() {
        let g = PinGeometry(pins: [], rect: plot())
        XCTAssertGreaterThan(
            g.screen(of: CGPoint(x: 0.3, y: 0.1)).y,
            g.screen(of: CGPoint(x: 0.3, y: 0.7)).y
        )
    }

    /// A pin is grabbed by its handle — where it has been dragged to — not by
    /// its origin. The origin says where the colour was; the handle is the
    /// thing you move.
    func testAPinIsGrabbedByItsHandle() {
        let dragged = PinValue(at: CGPoint(x: 0.2, y: 0.65), to: CGPoint(x: 0.45, y: 0.35),
                               chromaRange: 0.04, tonalLow: 1, tonalHigh: 1,
                               tonalPivot: 0.5, exposure: 0)
        let g = PinGeometry(pins: [dragged], rect: plot())
        XCTAssertEqual(g.grabbed(at: g.screen(of: dragged.to)), 0)
        XCTAssertNil(g.grabbed(at: g.screen(of: dragged.at)), "grabbed by the origin")
    }

    func testAPinIsOnlyGrabbedWhenItWasAimedAt() {
        let p = PinValue.placed(at: CGPoint(x: 0.33, y: 0.35))
        let g = PinGeometry(pins: [p], rect: plot())
        let on = g.screen(of: p.to)
        XCTAssertEqual(g.grabbed(at: on), 0)
        XCTAssertNil(g.grabbed(at: CGPoint(x: on.x + 40, y: on.y + 40)))
    }

    func testTheNearestOfSeveralPinsIsTheOneGrabbed() {
        let a = PinValue.placed(at: CGPoint(x: 0.2, y: 0.2))
        let b = PinValue.placed(at: CGPoint(x: 0.5, y: 0.5))
        let g = PinGeometry(pins: [a, b], rect: plot())
        XCTAssertEqual(g.grabbed(at: g.screen(of: b.to)), 1)
        XCTAssertEqual(g.grabbed(at: g.screen(of: a.to)), 0)
    }

    /// How far a pin reaches, drawn — the control people forget is there until
    /// they can see it.
    func testAPinsReachIsDrawnInTheSameUnitsAsThePlot() {
        let g = PinGeometry(pins: [], rect: plot())
        // A range spanning the whole plot should be the plot's own width.
        let whole = PinGeometry.plotSpan - PinGeometry.plotMin
        XCTAssertEqual(g.reach(chromaRange: whole), 200, accuracy: 0.001)
        XCTAssertEqual(g.reach(chromaRange: whole / 2), 100, accuracy: 0.001)
    }

    func testAZeroSizedPlotDoesNotDivideByZero() {
        let g = PinGeometry(pins: [], rect: .zero)
        let p = g.chromaticity(CGPoint(x: 5, y: 5))
        XCTAssertFalse(p.x.isNaN)
        XCTAssertFalse(p.y.isNaN)
    }
}
