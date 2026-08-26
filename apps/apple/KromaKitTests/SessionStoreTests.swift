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

    func testARowCanBeAddedRemovedAndReordered() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        let pinned = store.snapshot.rows.count

        let grain = try XCTUnwrap(store.addEffect("grain"))
        XCTAssertEqual(store.snapshot.rows.count, pinned + 1)
        XCTAssertEqual(store.snapshot.rows.last?.effect, "grain")

        let halation = try XCTUnwrap(store.addEffect("halation"))
        XCTAssertEqual(store.snapshot.rows.count, pinned + 2)

        // Reordering moves it within the stack, and the stack is the document —
        // grain under halation is a different photograph from halation under
        // grain.
        store.moveRow(halation, to: UInt32(pinned))
        let order = store.snapshot.rows.map(\.effect)
        XCTAssertLessThan(
            order.firstIndex(of: "halation")!,
            order.firstIndex(of: "grain")!
        )

        store.removeRow(grain)
        XCTAssertEqual(store.snapshot.rows.count, pinned + 1)
        XCTAssertFalse(store.snapshot.rows.contains { $0.effect == "grain" })
    }

    func testARowCanBeSwitchedOffWithoutBeingRemoved() throws {
        // Bypassing a row is how you find out what it was doing. Removing it
        // and adding it back is not the same thing — it loses the parameters.
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        let row = try XCTUnwrap(store.addEffect("grain"))
        // Grain's own float, not the row's opacity, and moved off its default
        // of 0.66 so the assertion below cannot pass by accident.
        store.setFloat(row: row, key: "size", value: 0.5)

        store.setRowEnabled(row, false)
        XCTAssertEqual(store.snapshot.rows.first { $0.id == row }?.enabled, false)

        store.setRowEnabled(row, true)
        let back = try XCTUnwrap(store.snapshot.rows.first { $0.id == row })
        XCTAssertTrue(back.enabled)
        // Unwrapped rather than compared as an optional: `XCTAssertEqual` with
        // an accuracy takes a `FloatingPoint`, and `Float?` is not one.
        let size = try XCTUnwrap(back.params["size"]?.floatValue)
        XCTAssertEqual(size, 0.5, accuracy: 0.0001)
    }

    func testAPinnedRowIsNotOfferedForRemoval() throws {
        // The pinned rows are the fixed panels of the colour page. Removing one
        // would leave a document a fresh one could not be, and the inspector
        // with a hole in it.
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        let pinned = try XCTUnwrap(store.snapshot.rows.first { $0.pinned })
        XCTAssertFalse(store.canRemove(pinned))

        let added = try XCTUnwrap(store.addEffect("grain"))
        let row = try XCTUnwrap(store.snapshot.rows.first { $0.id == added })
        XCTAssertTrue(store.canRemove(row))
    }

    func testACropDragDrawsWhatTheEngineAcceptedAndNotWhatWasAsked() throws {
        // The crop's version of `testADragDoesNotRefreshUntilItEnds`, with the
        // extra thing that makes this path different: the value the overlay
        // reads mid-drag is the engine's corrected one, because the snapshot it
        // would otherwise read is deliberately behind.
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 64, height: 64)
        XCTAssertTrue(store.geometry.isIdentity)
        let atRest = store.snapshot.version

        store.beginInteraction("Crop")
        store.setGeometry(
            GeometryValue(
                centre: CGPoint(x: 0.9, y: 0.9), size: CGSize(width: 0.5, height: 0.5),
                angle: 0, turns: 0, flipH: false, flipV: false, aspect: .free
            )
        )
        XCTAssertEqual(store.snapshot.version, atRest, "refreshed during a drag")
        XCTAssertNil(store.problem)

        // Mid-drag, and already corrected: a store that handed back the
        // proposal would draw a rectangle hanging off the corner and then jump
        // when the gesture ended.
        let drawn = store.geometry
        XCTAssertNotEqual(drawn.centre, CGPoint(x: 0.9, y: 0.9))
        XCTAssertLessThanOrEqual(abs(drawn.centre.x) + drawn.size.width / 2, 0.5 + 1e-4)

        store.endInteraction()
        XCTAssertGreaterThan(store.snapshot.version, atRest)
        // And nothing jumped: what was drawn mid-drag is what the document
        // holds now.
        XCTAssertEqual(store.geometry.centre.x, drawn.centre.x, accuracy: 1e-6)
        XCTAssertEqual(store.geometry.size.width, drawn.size.width, accuracy: 1e-6)

        store.resetGeometry()
        XCTAssertTrue(store.geometry.isIdentity)
    }

    /// A 1x1 white PNG, so the test needs no fixture file on disk.
    static let onePixelPNG = """
    iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==
    """
}
