import XCTest

// No `import KromaKit`: these sources compile *into* the test bundle, whose
// module is named KromaKit (PRODUCT_MODULE_NAME in project.yml), so the tests
// are already inside the module they exercise. `@testable import KromaKit`
// here compiles, but the compiler warns that it is ignoring an import of the
// file's own module — and internal access works without it.
final class EngineTests: XCTestCase {
    func testAFreshSessionOpensTheTestChartAndHasThePinnedRows() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        // new_document seeds the pinned rows, so an opened photograph is never
        // an empty stack. Eleven of them at the time of writing.
        XCTAssertEqual(session.rowCount, 11)
    }

    func testTheEngineReportsItsVersion() {
        XCTAssertFalse(Engine.version.isEmpty)
        XCTAssertNotEqual(Engine.version, "unknown")
    }

    func testAParameterTheEffectDoesNotHaveIsRefusedWithAMessage() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        let row = try session.addEffect("sharpen")

        XCTAssertThrowsError(try session.setFloat(row: row, key: "not_a_parameter", value: 1)) {
            error in
            let text = String(describing: error)
            XCTAssertTrue(
                text.contains("not_a_parameter"),
                "a refusal nobody can act on: \(text)"
            )
        }
    }

    func testADragBracketedByAnInteractionIsOneUndoStep() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        let row = try session.addEffect("sharpen")

        session.beginInteraction("Amount")
        for i in 1...60 {
            try session.setFloat(row: row, key: "amount", value: Float(i) * 0.01)
        }
        session.endInteraction()

        // One undo puts the whole drag back, not one frame of it — back to
        // 1.8, which is where `add_effect` seeded it. Not 0: sharpen's amount
        // *defaults* to 1.8 and is *neutral* at 0, and the two are different
        // questions. A freshly added Sharpen should sharpen; the neutral is
        // only where the slider's fill grows from.
        XCTAssertTrue(try session.undo())
        let snapshot = try session.snapshot()
        let amount = try XCTUnwrap(
            snapshot.rows.first { $0.id == row }?.params["amount"]?.floatValue
        )
        XCTAssertEqual(amount, 1.8, accuracy: 0.0001, "one undo left the drag partly applied")

        // And a second undo removes the row, so the drag really was one step.
        XCTAssertTrue(try session.undo())
        XCTAssertFalse(session.canUndo)
    }

    func testAnUnknownEffectIsRefused() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        XCTAssertThrowsError(try session.addEffect("not_an_effect"))
    }
}
