import XCTest
// Same module as the code under test; see EngineTests.swift.

/// The GPU's name, which the status bar draws last and quietly.
///
/// Nobody reads it until something has gone wrong, and then it is the first
/// thing worth knowing — the maximum texture dimension in it is what decides
/// whether a given photograph opens at all.
@MainActor
final class GpuNameTests: XCTestCase {
    /// Nil until a device exists, and asking must not create one.
    ///
    /// A session that has not drawn has no device. Acquiring one to answer a
    /// question about it would make reading a status-bar label the most
    /// expensive thing in the frame — and would do it on the first layout,
    /// before there is a window to draw into.
    func testThereIsNoNameBeforeAnythingIsDrawn() throws {
        let store = try XCTUnwrap(SessionStore())
        XCTAssertNil(store.gpuName)

        store.openTestChart()
        XCTAssertFalse(store.snapshot.rows.isEmpty, "the chart did not open")
        XCTAssertNil(store.gpuName, "opening a photograph acquired a device")
    }

    /// And asking repeatedly is safe. The string crosses the ABI as an
    /// allocation the caller frees, so a leak or a double free would show up
    /// here rather than after an afternoon's grading.
    func testAskingRepeatedlyIsHarmless() throws {
        let store = try XCTUnwrap(SessionStore())
        for _ in 0..<200 {
            XCTAssertNil(store.gpuName)
        }
        XCTAssertNil(store.problem)
    }
}
