import XCTest
// Same module as the code under test; see EngineTests.swift.

final class SliderGeometryTests: XCTestCase {
    private let bounds = Bounds(min: -5, max: 5, default: 0, neutral: 0)

    func testTheEndsAndTheMiddleLandWhereTheyShould() {
        let g = SliderGeometry(bounds: bounds, width: 100)
        XCTAssertEqual(g.position(of: -5), 0, accuracy: 0.001)
        XCTAssertEqual(g.position(of: 5), 100, accuracy: 0.001)
        XCTAssertEqual(g.position(of: 0), 50, accuracy: 0.001)
    }

    func testAPositionBecomesAValueAndBack() {
        let g = SliderGeometry(bounds: bounds, width: 100)
        XCTAssertEqual(g.value(at: 75), 2.5, accuracy: 0.001)
        XCTAssertEqual(g.position(of: g.value(at: 30)), 30, accuracy: 0.001)
    }

    func testADragPastEitherEndStops() {
        let g = SliderGeometry(bounds: bounds, width: 100)
        XCTAssertEqual(g.value(at: -40), -5)
        XCTAssertEqual(g.value(at: 400), 5)
    }

    func testTheFillGrowsFromTheNeutralPointRatherThanFromTheLeft() {
        // Resolve draws the track filled from where "no change" sits, so a
        // negative exposure reads as a bar going the other way. Filling from
        // the left would say every value is a positive amount of something.
        let g = SliderGeometry(bounds: bounds, width: 100)
        let below = g.fill(for: -2.5)
        XCTAssertEqual(below.origin, 25, accuracy: 0.001)
        XCTAssertEqual(below.width, 25, accuracy: 0.001)

        let above = g.fill(for: 2.5)
        XCTAssertEqual(above.origin, 50, accuracy: 0.001)
        XCTAssertEqual(above.width, 25, accuracy: 0.001)
    }

    func testAnOffCentreNeutralStillAnchorsTheFill() {
        // Not every neutral is the middle: a Gain wheel rests at one, and
        // Temperature rests at 6500 on a scale that starts at 2000.
        let temp = Bounds(min: 2000, max: 11000, default: 6500, neutral: 6500)
        let g = SliderGeometry(bounds: temp, width: 90)
        XCTAssertEqual(g.position(of: 6500), 45, accuracy: 0.001)
        XCTAssertEqual(g.fill(for: 6500).width, 0, accuracy: 0.001)
    }

    func testAZeroWidthTrackDoesNotDivideByZero() {
        // Views are laid out at zero width for one pass. A NaN here becomes a
        // control that never draws again.
        let g = SliderGeometry(bounds: bounds, width: 0)
        XCTAssertEqual(g.position(of: 1), 0)
        XCTAssertEqual(g.value(at: 10), bounds.min)
        XCTAssertFalse(g.position(of: 1).isNaN)
    }

    func testADegenerateRangeDoesNotDivideByZero() {
        let flat = Bounds(min: 1, max: 1, default: 1, neutral: 1)
        let g = SliderGeometry(bounds: flat, width: 100)
        XCTAssertFalse(g.position(of: 1).isNaN)
        XCTAssertEqual(g.value(at: 50), 1)
    }
}
