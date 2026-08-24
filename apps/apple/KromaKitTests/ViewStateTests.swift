import XCTest
// Same module as the code under test; see EngineTests.swift.

final class ViewStateTests: XCTestCase {
    func testAFittedViewShowsTheWholeFrame() {
        var v = ViewState()
        XCTAssertEqual(v.zoom, 1, accuracy: 0.0001)
        XCTAssertEqual(v.pan.x, 0, accuracy: 0.0001)
        XCTAssertEqual(v.pan.y, 0, accuracy: 0.0001)
        v.fit()
        XCTAssertEqual(v.zoom, 1, accuracy: 0.0001)
    }

    func testZoomingHoldsThePointUnderTheCursorStill() {
        // The whole reason to anchor a zoom: the pixel you are looking at is
        // the one you want to keep looking at. A zoom that anchors at the
        // centre walks the thing you were inspecting off the screen.
        var v = ViewState()
        let cursor = CGPoint(x: 0.25, y: 0.75)
        let before = v.frameLocation(of: cursor)
        v.zoom(by: 2.5, at: cursor)
        let after = v.frameLocation(of: cursor)
        XCTAssertEqual(after.x, before.x, accuracy: 0.001)
        XCTAssertEqual(after.y, before.y, accuracy: 0.001)
    }

    func testZoomStopsAtBothEnds() {
        // Out beyond fit is a picture floating in a void; in beyond thirty-two
        // is a single pixel filling a screen. Neither is a view of anything.
        var v = ViewState()
        v.zoom(by: 0.001, at: CGPoint(x: 0.5, y: 0.5))
        XCTAssertEqual(v.zoom, 1, accuracy: 0.0001)
        v.zoom(by: 10_000, at: CGPoint(x: 0.5, y: 0.5))
        XCTAssertEqual(v.zoom, ViewState.maxZoom, accuracy: 0.0001)
    }

    func testPanningCannotPushTheFrameOffScreen()  {
        // At any zoom the visible rectangle stays inside the picture, so there
        // is never a band of nothing along an edge.
        var v = ViewState()
        v.zoom(by: 4, at: CGPoint(x: 0.5, y: 0.5))
        v.pan(by: CGSize(width: 100, height: 100))
        XCTAssertGreaterThanOrEqual(v.region.origin.x, 0)
        XCTAssertGreaterThanOrEqual(v.region.origin.y, 0)
        XCTAssertLessThanOrEqual(v.region.maxX, 1.0001)
        XCTAssertLessThanOrEqual(v.region.maxY, 1.0001)
    }

    func testAtFitThereIsNothingToPan() {
        var v = ViewState()
        v.pan(by: CGSize(width: 250, height: -80))
        XCTAssertEqual(v.region.origin.x, 0, accuracy: 0.0001)
        XCTAssertEqual(v.region.origin.y, 0, accuracy: 0.0001)
        XCTAssertEqual(v.region.width, 1, accuracy: 0.0001)
    }

    func testTheRegionShrinksAsYouZoomIn() {
        var v = ViewState()
        v.zoom(by: 4, at: CGPoint(x: 0.5, y: 0.5))
        XCTAssertEqual(v.region.width, 0.25, accuracy: 0.0001)
        XCTAssertEqual(v.region.height, 0.25, accuracy: 0.0001)
    }
}
