import XCTest
// Same module as the code under test; see EngineTests.swift.

/// Fit, 100%, and what the zoom is worth.
///
/// The scale itself is the engine's — it needs the layer's size, which only the
/// engine has — and its arithmetic is checked on the Rust side where a headless
/// test can reach it. What is checked here is this side's part: that a session
/// with no layer says so rather than inventing a number, and that Fit knows
/// when it has nothing to do.
@MainActor
final class ZoomReadoutTests: XCTestCase {
    private func opened() throws -> SessionStore {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        return store
    }

    /// With no layer attached there is no viewport, so there is no scale.
    ///
    /// The button is greyed by exactly this, and the readout draws an em dash:
    /// a plausible "100%" would be read as an answer.
    func testThereIsNoScaleWithoutALayer() throws {
        let store = try opened()
        XCTAssertNil(store.viewScale, "a scale was reported with nothing to measure against")
    }

    /// And 100% does nothing rather than dividing by a scale it does not have.
    func testZoomingToActualPixelsWithoutALayerDoesNothing() throws {
        let store = try opened()
        XCTAssertTrue(store.isFit)
        store.zoomToActualPixels()
        XCTAssertTrue(store.isFit, "the view moved on a scale that does not exist")
        XCTAssertNil(store.problem)
    }

    /// Fit is about the *view*, not the layer, so it answers with or without
    /// one — which is what lets the button be greyed before anything is drawn.
    func testFitKnowsWhenItHasNothingToDo() throws {
        let store = try opened()
        XCTAssertTrue(store.isFit, "a fresh view is not fitted")

        store.zoom(by: 4, at: CGPoint(x: 0.5, y: 0.5))
        XCTAssertFalse(store.isFit, "zooming in left the view fitted")

        store.fitView()
        XCTAssertTrue(store.isFit, "Fit did not fit")
    }

    /// Fitting is a property of the window and never an edit — undo has nothing
    /// to put back, and the document's version must not move.
    func testFittingIsNotAnEdit() throws {
        let store = try opened()
        let version = store.snapshot.version
        store.zoom(by: 4, at: CGPoint(x: 0.5, y: 0.5))
        store.fitView()
        XCTAssertEqual(store.snapshot.version, version, "moving the view edited the document")
        XCTAssertFalse(store.canUndo, "the view put something on the undo stack")
    }
}
