import AppKit
import XCTest
// Same module as the code under test; see EngineTests.swift.

/// The explicit save: a `.peproj` beside the photograph.
///
/// A sidecar is a decision — *this* is the edit, keep it, move it with the
/// photograph — where the autosave is only where you happened to stop. Both
/// wrappers existed on this side for a long time with nothing calling them.
@MainActor
final class SaveEditTests: XCTestCase {
    /// One real PNG in a temporary folder, and the folder to clean up.
    private func photograph(named: String) throws -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("kroma-\(named)", isDirectory: true)
        try? FileManager.default.removeItem(at: dir)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let rep = try XCTUnwrap(
            NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: 8, pixelsHigh: 8, bitsPerSample: 8,
                samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
        let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
        let file = dir.appendingPathComponent("shot.png")
        try png.write(to: file)
        return file
    }

    /// Saved, thrown away, loaded back. The whole point of the file.
    func testAnEditSurvivesBeingSavedAndLoadedBack() throws {
        let file = try photograph(named: "sidecar-round-trip")
        defer { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }

        let store = try XCTUnwrap(SessionStore())
        store.openPaths([file])
        XCTAssertNil(store.problem, store.problem ?? "")

        XCTAssertNotNil(store.addEffect("sharpen"), store.problem ?? "refused")
        let withEffect = store.snapshot.rows.count
        store.saveEdit()
        XCTAssertNil(store.problem, store.problem ?? "")
        XCTAssertEqual(store.notice, "saved shot.peproj")

        let sidecar = file.deletingPathExtension().appendingPathExtension("peproj")
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: sidecar.path),
            "nothing was written beside the photograph")

        // Throw the edit away, then pull the sidecar back over it.
        store.undo()
        XCTAssertEqual(store.snapshot.rows.count, withEffect - 1)

        store.loadEdit(sidecar)
        XCTAssertNil(store.problem, store.problem ?? "")
        XCTAssertEqual(store.notice, "loaded shot.peproj")
        XCTAssertEqual(store.snapshot.rows.count, withEffect, "the saved edit did not come back")
        XCTAssertNotNil(store.snapshot.rows.first { $0.effect == "sharpen" && !$0.pinned })
    }

    /// Loading is one undo step, so it can be taken back like any other edit.
    func testLoadingAnEditCanBeUndone() throws {
        let file = try photograph(named: "sidecar-undo")
        defer { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }

        let store = try XCTUnwrap(SessionStore())
        store.openPaths([file])
        store.addEffect("sharpen")
        store.saveEdit()
        let sidecar = file.deletingPathExtension().appendingPathExtension("peproj")

        store.undo()
        let without = store.snapshot.rows.count
        store.loadEdit(sidecar)
        XCTAssertEqual(store.snapshot.rows.count, without + 1)

        store.undo()
        XCTAssertEqual(store.snapshot.rows.count, without, "loading was not one step")
    }

    /// Every photograph of the set that has an edit, and nothing beside the
    /// ones that do not: a `.peproj` full of defaults is noise in a folder.
    func testSavingAllWritesOnlyThePhotographsThatHaveAnEdit() throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("kroma-save-all-swift", isDirectory: true)
        try? FileManager.default.removeItem(at: dir)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        for name in ["a", "b", "c"] {
            let rep = try XCTUnwrap(
                NSBitmapImageRep(
                    bitmapDataPlanes: nil, pixelsWide: 8, pixelsHigh: 8, bitsPerSample: 8,
                    samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
            let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
            try png.write(to: dir.appendingPathComponent("\(name).png"))
        }

        let store = try XCTUnwrap(SessionStore())
        store.openFolder(dir)
        XCTAssertNil(store.problem, store.problem ?? "")
        XCTAssertEqual(store.library.count, 3)

        store.addEffect("sharpen")
        store.focus(1)
        store.addEffect("dehaze")

        store.saveAllEdits()
        XCTAssertNil(store.problem, store.problem ?? "")
        XCTAssertEqual(store.notice, "saved 2 edits")

        let fm = FileManager.default
        XCTAssertTrue(fm.fileExists(atPath: dir.appendingPathComponent("a.peproj").path))
        XCTAssertTrue(fm.fileExists(atPath: dir.appendingPathComponent("b.peproj").path))
        XCTAssertFalse(
            fm.fileExists(atPath: dir.appendingPathComponent("c.peproj").path),
            "a file full of defaults was written beside an untouched photograph")
    }

    /// The built-in chart is not a file, so there is nothing to write a sidecar
    /// beside — which is what the menu item is greyed by.
    func testTheChartHasNothingToSaveBeside() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        XCTAssertNil(store.snapshot.path, "the chart claims a file")

        store.saveEdit()
        XCTAssertNotNil(store.problem, "a sidecar was written for a photograph with no file")
        XCTAssertNil(store.notice)
    }
}
