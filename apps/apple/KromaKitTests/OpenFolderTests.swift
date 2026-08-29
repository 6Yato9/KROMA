import AppKit
import XCTest
// Same module as the code under test; see EngineTests.swift.

/// Opening a folder, which is what turns a photograph into a *set*.
///
/// Worth its own file because of what depends on it: until this existed the
/// Mac could only open one photograph at a time, so the filmstrip had nothing
/// to show, Export All had nothing to run on and Paste to All was permanently
/// greyed — three finished features nothing could reach.
@MainActor
final class OpenFolderTests: XCTestCase {
    /// A folder with `count` real PNGs in it, and a text file that is not one.
    private func folder(named: String, count: Int) throws -> URL {
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
        try Data("not a photograph".utf8).write(to: dir.appendingPathComponent("notes.txt"))
        return dir
    }

    /// The whole point: a folder becomes a set, and the set is what the strip
    /// draws. The text file is not counted.
    func testAFolderBecomesASetTheFilmstripCanShow() throws {
        let dir = try folder(named: "set", count: 3)
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = try XCTUnwrap(SessionStore())
        XCTAssertFalse(
            Filmstrip.isWorthShowing(count: store.library.count),
            "there is a set before one was opened")

        store.openFolder(dir)
        XCTAssertNil(store.problem, store.problem ?? "")
        XCTAssertEqual(store.library.count, 3, "the text file was counted as a photograph")
        XCTAssertTrue(
            Filmstrip.isWorthShowing(count: store.library.count),
            "a folder of three is still not a set the strip will draw")
        XCTAssertEqual(store.notice, "opened 3 photographs")
    }

    /// One photograph is a set of one, said in the singular. A folder that
    /// opened is otherwise indistinguishable from one that did not.
    func testOneIsSaidInTheSingular() throws {
        let dir = try folder(named: "single", count: 1)
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = try XCTUnwrap(SessionStore())
        store.openFolder(dir)
        XCTAssertNil(store.problem)
        XCTAssertEqual(store.notice, "opened 1 photograph")
    }

    /// A folder with nothing readable in it is refused by name, and the refusal
    /// goes to `problem` — not to `notice`, which would draw it as a success.
    func testAFolderOfNothingIsRefusedByName() throws {
        let dir = try folder(named: "empty", count: 0)
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = try XCTUnwrap(SessionStore())
        store.openFolder(dir)
        let problem = try XCTUnwrap(store.problem, "a folder with no photographs was opened")
        XCTAssertTrue(
            problem.contains("no photographs in"),
            "the refusal does not say what was wrong: \(problem)")
        XCTAssertNil(store.notice)
        XCTAssertEqual(store.library.count, 0)
    }
}
