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
}
