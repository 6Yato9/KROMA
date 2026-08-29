import AppKit
import XCTest
// Same module as the code under test; see EngineTests.swift.

/// Moving through the set without the filmstrip.
///
/// The strip can be put away, and until this existed clicking a cell in it was
/// the only way to reach another photograph.
@MainActor
final class SetNavigationTests: XCTestCase {
    private func folder(_ named: String, count: Int) throws -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("kroma-\(named)", isDirectory: true)
        try? FileManager.default.removeItem(at: dir)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        for i in 0..<count {
            let rep = try XCTUnwrap(
                NSBitmapImageRep(
                    bitmapDataPlanes: nil, pixelsWide: 8, pixelsHigh: 8, bitsPerSample: 8,
                    samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
            let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
            try png.write(to: dir.appendingPathComponent("photo\(i).png"))
        }
        return dir
    }

    private func opened(_ named: String, count: Int) throws -> (SessionStore, URL) {
        let dir = try folder(named, count: count)
        let store = try XCTUnwrap(SessionStore())
        store.openFolder(dir)
        XCTAssertNil(store.problem, store.problem ?? "")
        return (store, dir)
    }

    func testWalkingForwardAndBackThroughASet() throws {
        let (store, dir) = try opened("nav-walk", count: 3)
        defer { try? FileManager.default.removeItem(at: dir) }

        XCTAssertEqual(store.library.current, 0)
        store.showNext()
        XCTAssertEqual(store.library.current, 1)
        store.showNext()
        XCTAssertEqual(store.library.current, 2)
        store.showPrevious()
        XCTAssertEqual(store.library.current, 1)
    }

    /// Clamped, not wrapping. A set that wraps takes you back to the first
    /// photograph when you thought you were at the last.
    func testTheEndsAreClampedAndSaySo() throws {
        let (store, dir) = try opened("nav-clamp", count: 2)
        defer { try? FileManager.default.removeItem(at: dir) }

        XCTAssertFalse(store.hasPrevious, "there is something before the first")
        store.showPrevious()
        XCTAssertEqual(store.library.current, 0, "moving back from the first wrapped")

        store.showNext()
        XCTAssertEqual(store.library.current, 1)
        XCTAssertFalse(store.hasNext, "there is something after the last")
        store.showNext()
        XCTAssertEqual(store.library.current, 1, "moving on from the last wrapped")
    }

    /// With no set there is nowhere to go, and both items are greyed rather
    /// than doing nothing when pressed.
    func testTheChartHasNowhereToGo() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        XCTAssertFalse(store.hasNext)
        XCTAssertFalse(store.hasPrevious)
        store.showNext()
        store.showPrevious()
        XCTAssertNil(store.problem, "moving through a set that is not there was an error")
    }
}
