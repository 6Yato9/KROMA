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
}
