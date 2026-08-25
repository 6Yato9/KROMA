import XCTest

// Same module as the code under test; see EngineTests.swift.
final class ScopesTests: XCTestCase {
    private func measured() throws -> (Session, Scopes) {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        try session.measureScopes(width: 64, height: 48)
        return (session, try XCTUnwrap(try session.scopes()))
    }

    func testAHistogramComesBackWithEveryPixelCounted() throws {
        let (session, scopes) = try measured()
        XCTAssertEqual(scopes.histogram.total, 64 * 48)
        XCTAssertEqual(scopes.histogram.red.count, 256)
        XCTAssertEqual(scopes.histogram.luma.count, 256)
        XCTAssertEqual(scopes.histogram.red.reduce(0, +), 64 * 48)
        XCTAssertGreaterThan(scopes.histogram.peak, 0)
        // A colour chart binned into one level would mean the measurement is
        // reading a blank rather than the picture.
        XCTAssertGreaterThan(scopes.histogram.luma.filter { $0 > 0 }.count, 1)
        // The clipping warning reads the same measurement, so it is a real
        // fraction once there is one rather than the ABI's negative sentinel.
        let overWhite = try XCTUnwrap(session.overWhiteFraction())
        XCTAssertGreaterThanOrEqual(overWhite, 0)
        XCTAssertLessThanOrEqual(overWhite, 1)
    }

    func testAWaveformIsShapedColumnsByLevels() throws {
        let (_, scopes) = try measured()
        XCTAssertEqual(scopes.waveform.columns, 64)
        XCTAssertEqual(scopes.waveform.levels, 256)
        XCTAssertEqual(scopes.waveform.plane(.luma).count, 64 * 256)
        // Each column holds exactly as many samples as the frame had rows.
        let column0 = (0..<256).map { scopes.waveform.at(.luma, column: 0, level: $0) }
        XCTAssertEqual(column0.reduce(0, +), 48)
        // And that row count is what a cell is drawn against, so a full cell
        // reads as 1 without the view doing the arithmetic.
        XCTAssertEqual(scopes.waveform.fullScale, 48)
    }

    func testAVectorscopeIsSquare() throws {
        let (_, scopes) = try measured()
        XCTAssertEqual(scopes.vectorscope.width, 256)
        XCTAssertEqual(scopes.vectorscope.height, 256)
        XCTAssertEqual(scopes.vectorscope.counts.count, 256 * 256)
    }

    func testTheGenerationSaysWhenToReadAgain() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        XCTAssertEqual(session.scopeGeneration(), 0)
        try session.measureScopes(width: 64, height: 48)
        let first = session.scopeGeneration()
        XCTAssertGreaterThan(first, 0)
        try session.measureScopes(width: 64, height: 48)
        XCTAssertGreaterThan(session.scopeGeneration(), first)
    }

    func testAnEditThrowsTheMeasurementAway() throws {
        // The counts describe a particular grade. Drawing numbers measured
        // before an edit would show a scope of a picture that is not on screen.
        let (session, _) = try measured()
        let row = try session.addEffect("exposure")
        try session.setFloat(row: row, key: "ev", value: 1.5)
        XCTAssertNil(try session.scopes(), "stale counts survived an edit")
    }

    func testReadingBeforeMeasuringGivesNothingRatherThanZeroes() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        XCTAssertNil(try session.scopes())
        XCTAssertNil(session.overWhiteFraction())
    }

    @MainActor
    func testTheStoreCopiesTheCountsOnceAndDropsThemOnAnEdit() throws {
        // The store holds the copy so a body evaluation never makes one. The
        // generation is what lets it: same number, same arrays, no 2.6 MB.
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 64, height: 64)
        XCTAssertNil(store.scopes, "counts appeared before anything was measured")

        store.measureScopes(width: 64, height: 48)
        let first = try XCTUnwrap(store.scopes)
        XCTAssertEqual(first.histogram.total, 64 * 48)
        XCTAssertNil(store.problem)

        // An edit does not advance the generation, so a store that watched only
        // that number would keep serving these. It must not.
        let row = try XCTUnwrap(store.addEffect("exposure"))
        store.setFloat(row: row, key: "ev", value: 1.5)
        XCTAssertNil(store.scopes, "stale counts survived an edit")

        store.measureScopes(width: 64, height: 48)
        let second = try XCTUnwrap(store.scopes)
        XCTAssertGreaterThan(second.generation, first.generation)
    }
}
