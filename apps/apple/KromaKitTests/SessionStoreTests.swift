import XCTest
// Same module as the code under test; see EngineTests.swift.

@MainActor
final class SessionStoreTests: XCTestCase {
    func testOpeningAChartFillsTheSnapshot() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        XCTAssertTrue(store.snapshot.isOpen)
        XCTAssertEqual(store.snapshot.rows.count, 11)
        XCTAssertNil(store.problem)
    }

    func testAStructuralEditRefreshesTheSnapshotImmediately() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        let before = store.snapshot.version
        store.addEffect("sharpen")
        XCTAssertGreaterThan(store.snapshot.version, before)
        XCTAssertEqual(store.snapshot.rows.count, 12)
    }

    func testADragDoesNotRefreshUntilItEnds() throws {
        // One FFI call per frame and one snapshot per drag. Refreshing mid-drag
        // would ask SwiftUI to diff the whole document sixty times a second
        // while a finger is down, which is the cost this design exists to avoid.
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        let row = try XCTUnwrap(store.addEffect("sharpen"))
        let atRest = store.snapshot.version

        store.beginInteraction("Amount")
        for i in 1...30 {
            store.setFloat(row: row, key: "amount", value: Float(i) * 0.01)
        }
        XCTAssertEqual(store.snapshot.version, atRest, "refreshed during a drag")

        store.endInteraction()
        XCTAssertGreaterThan(store.snapshot.version, atRest)
        // `Float(30) * 0.01` does not round to the same bit pattern as the
        // literal `0.3` — that is ordinary Float rounding, not a bug in the
        // store, so this compares within a tolerance rather than exactly.
        let amount = try XCTUnwrap(store.snapshot.rows.first { $0.id == row }?.params["amount"]?.floatValue)
        XCTAssertEqual(amount, 0.3, accuracy: 0.0001)
    }

    func testARefusalIsReportedRatherThanThrownAway() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        store.addEffect("not_an_effect")
        XCTAssertNotNil(store.problem, "the engine refused and nobody was told")
    }

    func testTheRegistryLoadsOnce() throws {
        let store = try XCTUnwrap(SessionStore())
        XCTAssertEqual(store.registry.effects.count, 30)
    }
}
