import XCTest
// Same module as the code under test; see EngineTests.swift.

final class WheelGeometryTests: XCTestCase {
    private let bounds = Bounds(min: -1, max: 1, default: 0, neutral: 0)

    func testTheCentreIsNoChange() {
        // A wheel at rest sits at its neutral in every channel, and the handle
        // sits in the middle. Resolve's Gain rests at one and Offset at
        // twenty-five, so "no change" is not "zero" and the centre is not the
        // bottom of the range.
        let g = WheelGeometry(bounds: bounds, radius: 50)
        let rgb = g.rgb(at: .zero)
        XCTAssertEqual(rgb[0], 0, accuracy: 0.0001)
        XCTAssertEqual(rgb[1], 0, accuracy: 0.0001)
        XCTAssertEqual(rgb[2], 0, accuracy: 0.0001)
    }

    func testAGainWheelRestsAtOneInTheCentre() {
        let gain = Bounds(min: 0.01, max: 16, default: 1, neutral: 1)
        let g = WheelGeometry(bounds: gain, radius: 50)
        for c in g.rgb(at: .zero) {
            XCTAssertEqual(c, 1, accuracy: 0.0001)
        }
    }

    func testPullingTowardsRedRaisesRedAndLowersTheOthers() {
        // The three channels sit at 90, 210 and 330 degrees. Dragging towards
        // one of them has to raise it and lower the other two, or the wheel is
        // a brightness control with extra steps.
        let g = WheelGeometry(bounds: bounds, radius: 50)
        let towardsRed = g.point(forAngle: WheelGeometry.redAngle, distance: 25)
        let rgb = g.rgb(at: towardsRed)
        XCTAssertGreaterThan(rgb[0], 0)
        XCTAssertLessThan(rgb[1], 0)
        XCTAssertLessThan(rgb[2], 0)
    }

    func testTheThreeChannelsSumToNothingSoTheWheelDoesNotShiftBrightness() {
        // A colour wheel moves hue and leaves level alone; the ring beside it
        // is what moves level. If the three did not cancel, every hue push
        // would also be an exposure push.
        let g = WheelGeometry(bounds: bounds, radius: 50)
        for angle in stride(from: 0.0, to: 360.0, by: 15.0) {
            let rgb = g.rgb(at: g.point(forAngle: angle, distance: 30))
            XCTAssertEqual(rgb[0] + rgb[1] + rgb[2], 0, accuracy: 0.0001,
                           "a push at \(angle) degrees changed the level")
        }
    }

    func testADragOutsideTheCircleStopsAtTheEdge() {
        let g = WheelGeometry(bounds: bounds, radius: 50)
        let far = g.rgb(at: CGPoint(x: 500, y: 0))
        let edge = g.rgb(at: CGPoint(x: 50, y: 0))
        for c in 0..<3 {
            XCTAssertEqual(far[c], edge[c], accuracy: 0.0001)
        }
    }

    func testAZeroRadiusWheelDoesNotDivideByZero() {
        // A view is laid out at zero size for at least one pass, and a NaN
        // that reaches a Shape is a control that never draws again.
        let g = WheelGeometry(bounds: bounds, radius: 0)
        for c in g.rgb(at: CGPoint(x: 10, y: 10)) {
            XCTAssertFalse(c.isNaN)
        }
        XCTAssertFalse(g.point(forAngle: 90, distance: 10).x.isNaN)
    }

    func testAValueRoundTripsBackToAPoint() {
        let g = WheelGeometry(bounds: bounds, radius: 50)
        let start = g.point(forAngle: 210, distance: 20)
        let back = g.point(for: g.rgb(at: start))
        XCTAssertEqual(back.x, start.x, accuracy: 0.5)
        XCTAssertEqual(back.y, start.y, accuracy: 0.5)
    }

    // ---- and what the disc under it is painted -----------------------------

    /// The disc agrees with the wheel it is drawn on.
    ///
    /// A colour wheel's background is not decoration: it is the map you aim at.
    /// `WheelGeometry` puts red up, green at two hundred and ten degrees and
    /// blue at three hundred and thirty; the disc was an `AngularGradient` of
    /// SwiftUI's primaries in gradient order, which starts at three o'clock —
    /// so red was drawn at the right of a wheel that raises red at the top, and
    /// a handle dragged into what looked like the reds raised something else.
    func testEachChannelsColourIsPaintedAtThatChannelsOwnAngle() {
        for (name, angle, channel) in [
            ("red", WheelGeometry.redAngle, 0),
            ("green", WheelGeometry.greenAngle, 1),
            ("blue", WheelGeometry.blueAngle, 2),
        ] {
            let painted = WheelView.discHue(atSweep: WheelView.sweep(forWheelAngle: angle))
            let parts = [Int(painted.r), Int(painted.g), Int(painted.b)]
            let others = parts.indices.filter { $0 != channel }.map { parts[$0] }
            XCTAssertGreaterThan(
                parts[channel], (others.max() ?? 0) + 40,
                "the disc paints \(painted) at \(angle)°, which is where the wheel "
                    + "pulls towards \(name)")
        }
    }

    /// The disc is one continuous circle that closes, and it is drawn from
    /// ``Ramp/hue`` — the same circle every Hue track in the application uses
    /// and the one the engine's fixture checks byte for byte. A wheel and a
    /// track disagreeing about where the cyans are is two hue circles in one
    /// panel.
    func testTheDiscClosesAndComesFromTheHueRamp() {
        XCTAssertEqual(
            WheelView.discHue(atSweep: 0), WheelView.discHue(atSweep: 1),
            "the disc does not come back round to where it started")

        // Red is at the wheel's red angle, and Ramp.hue's red is at zero. So
        // the sweep the disc paints red at must be the sweep that reads hue
        // zero off the ramp, whichever way round the two run.
        XCTAssertEqual(
            WheelView.discHue(atSweep: WheelView.sweep(forWheelAngle: WheelGeometry.redAngle)),
            Ramp.hue.at(0))
        XCTAssertNotEqual(
            WheelView.discHue(atSweep: 0), Ramp.hue.at(0),
            "three o'clock is a quarter turn from the wheel's red, not red itself")
    }
}
