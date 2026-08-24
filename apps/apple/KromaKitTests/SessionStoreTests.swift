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

    func testWorkInProgressComesBackWhenThePhotographIsReopened() throws {
        let tmp = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: tmp, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tmp) }

        // A real file, because the autosave store keys on the photograph's
        // path and refuses to hand one photograph's edit to another.
        let photo = tmp.appendingPathComponent("a.png")
        try Data(base64Encoded: Self.onePixelPNG)!.write(to: photo)

        let support = tmp.appendingPathComponent("support", isDirectory: true)

        let first = try XCTUnwrap(SessionStore())
        first.setSupportDirectory(support)
        first.open(photo)
        XCTAssertTrue(first.snapshot.isOpen, "the fixture did not open: \(first.problem ?? "")")
        let row = try XCTUnwrap(first.addEffect("sharpen"))
        first.setFloat(row: row, key: "amount", value: 1.5)
        first.flush()

        let second = try XCTUnwrap(SessionStore())
        second.setSupportDirectory(support)
        second.open(photo)
        let restored = try XCTUnwrap(
            second.snapshot.rows
                .first { $0.effect == "sharpen" }?
                .params["amount"]?.floatValue,
            "stopping cost something"
        )
        XCTAssertEqual(restored, 1.5, accuracy: 0.0001)
    }

    /// A 1x1 white PNG, so the test needs no fixture file on disk.
    static let onePixelPNG = """
    iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==
    """
}
