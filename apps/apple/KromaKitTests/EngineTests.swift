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

    func testACurveSurvivesTheRoundTrip() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        let snap = try session.snapshot()
        let curves = try XCTUnwrap(snap.rows.first { $0.effect == "curves" })

        try session.setCurve(
            row: curves.id, key: "luma",
            points: [CGPoint(x: 0, y: 0), CGPoint(x: 0.5, y: 0.7), CGPoint(x: 1, y: 1)]
        )

        let after = try session.snapshot()
        let row = try XCTUnwrap(after.rows.first { $0.id == curves.id })
        guard case let .curve(c) = try XCTUnwrap(row.params["luma"]) else {
            return XCTFail("luma is not a curve")
        }
        XCTAssertEqual(c.points.count, 3)
        XCTAssertEqual(c.points[1].y, 0.7, accuracy: 0.0001)
    }

    func testACurveWithOnePointIsRefusedRatherThanStored() throws {
        // The engine refuses it; this checks Swift surfaces the refusal as an
        // error rather than swallowing it, because the fallback the engine
        // would otherwise apply is a straight line the user did not draw.
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        let snap = try session.snapshot()
        let curves = try XCTUnwrap(snap.rows.first { $0.effect == "curves" })
        XCTAssertThrowsError(
            try session.setCurve(row: curves.id, key: "luma", points: [CGPoint(x: 0.5, y: 0.5)])
        )
    }

    func testAVertexSurvivesTheRoundTrip() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        let warper = try XCTUnwrap(try session.snapshot().rows.first { $0.effect == "colour_warper" })

        try session.setWarpVertex(row: warper.id, key: "hue_sat", col: 2, vertexRow: 3,
                                  offset: CGPoint(x: 0.25, y: -0.1))

        let after = try XCTUnwrap(try session.snapshot().rows.first { $0.id == warper.id })
        let w = try XCTUnwrap(after.params["hue_sat"]?.warpValue)
        XCTAssertEqual(w.at(col: 2, row: 3).x, 0.25, accuracy: 0.0001)
        XCTAssertEqual(w.at(col: 2, row: 3).y, -0.1, accuracy: 0.0001)
        XCTAssertFalse(w.isIdentity)

        try session.clearWarp(row: warper.id, key: "hue_sat")
        let cleared = try XCTUnwrap(try session.snapshot().rows.first { $0.id == warper.id })
        XCTAssertTrue(try XCTUnwrap(cleared.params["hue_sat"]?.warpValue).isIdentity)
    }

    func testAVertexTheGridDoesNotHaveIsRefused() throws {
        // The engine refuses it rather than dropping it. This checks Swift
        // surfaces the refusal instead of reporting a success that did nothing.
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        let warper = try XCTUnwrap(try session.snapshot().rows.first { $0.effect == "colour_warper" })
        XCTAssertThrowsError(
            try session.setWarpVertex(row: warper.id, key: "hue_sat", col: 99, vertexRow: 0,
                                      offset: CGPoint(x: 0.1, y: 0.1))
        )
    }

    func testAPinCanBePlacedDraggedShapedAndRemoved() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        let warper = try XCTUnwrap(try session.snapshot().rows.first { $0.effect == "colour_warper" })

        let i = try session.addPin(row: warper.id, key: "pins", at: CGPoint(x: 0.33, y: 0.35))
        XCTAssertEqual(i, 0)

        try session.movePin(row: warper.id, key: "pins", index: 0, to: CGPoint(x: 0.40, y: 0.30))
        try session.setPinShape(
            row: warper.id, key: "pins", index: 0,
            chromaRange: 0.12, tonalLow: 0.2, tonalHigh: 0.9, tonalPivot: 0.6, exposure: 0.75
        )

        let pins = try XCTUnwrap(
            try session.snapshot().rows.first { $0.id == warper.id }?.params["pins"]?.pinsValue
        )
        XCTAssertEqual(pins.count, 1)
        XCTAssertEqual(pins[0].at.x, 0.33, accuracy: 0.0001, "the origin stays put")
        XCTAssertEqual(pins[0].to.x, 0.40, accuracy: 0.0001)
        XCTAssertEqual(pins[0].exposure, 0.75, accuracy: 0.0001)
        XCTAssertFalse(pins[0].isNeutral)

        try session.removePin(row: warper.id, key: "pins", index: 0)
        XCTAssertTrue(
            try XCTUnwrap(
                try session.snapshot().rows.first { $0.id == warper.id }?.params["pins"]?.pinsValue
            ).isEmpty
        )
    }

    func testAPinThatIsNotThereIsRefused() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        let warper = try XCTUnwrap(try session.snapshot().rows.first { $0.effect == "colour_warper" })
        XCTAssertThrowsError(
            try session.movePin(row: warper.id, key: "pins", index: 0, to: CGPoint(x: 0.4, y: 0.4))
        )
    }

    func testANinthPinIsRefused() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        let warper = try XCTUnwrap(try session.snapshot().rows.first { $0.effect == "colour_warper" })
        for i in 0..<PinValue.maxPins {
            XCTAssertEqual(
                try session.addPin(row: warper.id, key: "pins",
                                   at: CGPoint(x: 0.1 * Double(i), y: 0.3)),
                i
            )
        }
        XCTAssertThrowsError(
            try session.addPin(row: warper.id, key: "pins", at: CGPoint(x: 0.5, y: 0.5))
        )
    }
}
