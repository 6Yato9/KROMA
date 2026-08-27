import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import XCTest

// Same module as the code under test; see EngineTests.swift.

/// The set of photographs, through the store the interface reads.
///
/// Against a real engine and real files on disc, because every interesting
/// thing here is the engine's: the worker thread that decodes a thumbnail, the
/// parked history that makes switching away and back safe, the refusal to open
/// a set of none. A stub would assert this side's opinion of all three.
///
/// The files are written with ImageIO rather than as hand-built PNG bytes or by
/// a Rust fixture step. It is the shortest honest route on this platform — the
/// system's own encoder, any size, any content, and no deflate implementation
/// written a second time — and what `pe_io` then decodes is an ordinary PNG.
@MainActor
final class LibraryTests: XCTestCase {

    // ---- what a set is ----------------------------------------------------

    func testASetOpensWithEveryPathAndTheFirstOnScreen() throws {
        let set = try Fixture(sizes: [(256, 128), (300, 100), (64, 64)])
        set.store.openPaths(set.paths)
        XCTAssertNil(set.store.problem)

        let library = set.store.library
        XCTAssertEqual(library.count, 3)
        XCTAssertEqual(library.current, 0)
        XCTAssertEqual(library.entries.map { $0.path.path }, set.paths.map { $0.path })
        XCTAssertEqual(library.entries.map(\.name), ["a.png", "b.png", "c.png"])
        XCTAssertEqual(library.entries.map(\.index), [0, 1, 2])
        // Nothing has been edited and no thumbnail has been asked for, so every
        // mark is clear — a set that reported otherwise on the way in would
        // draw a strip full of dots before anybody had touched it.
        XCTAssertEqual(library.entries.filter(\.edited).count, 0)
        XCTAssertEqual(library.entries.filter(\.failed).count, 0)
        XCTAssertEqual(library.entries.filter(\.hasThumbnail).count, 0)
        // And the first of them is the photograph on screen.
        XCTAssertTrue(set.store.snapshot.isOpen)
    }

    func testTheChartHasNoSetAndOnePhotographIsASetOfOne() throws {
        // Not an empty set: a session showing nothing has no set at all, and
        // the chart is not a file so it is not a set of one either. A strip of
        // no entries is the right thing to draw for both.
        let set = try Fixture(sizes: [(64, 64)])
        XCTAssertEqual(set.store.library, .empty)

        set.store.openTestChart(width: 64, height: 64)
        XCTAssertTrue(set.store.snapshot.isOpen)
        XCTAssertEqual(set.store.library, .empty)
        XCTAssertNil(set.store.library.current)

        // Opening one photograph *is* a set of one: `Session::open_path`
        // delegates to the list, so the strip has an entry to draw and the
        // interface has one path to answer with rather than two.
        set.store.open(set.paths[0])
        XCTAssertTrue(set.store.snapshot.isOpen)
        XCTAssertEqual(set.store.library.count, 1)
        XCTAssertEqual(set.store.library.current, 0)
        XCTAssertEqual(set.store.library.entries.first?.name, "a.png")

        // And the chart afterwards puts it back to nothing, rather than leaving
        // a strip pointing at the photograph that is no longer on screen.
        set.store.openTestChart(width: 64, height: 64)
        XCTAssertEqual(set.store.library, .empty)
    }

    func testAnEmptyListIsRefusedRatherThanOpeningASetOfNone() throws {
        let set = try Fixture(sizes: [(64, 64)])
        set.store.openPaths([])
        XCTAssertNotNil(set.store.problem, "the engine opened a set of no photographs")
        XCTAssertEqual(set.store.library, .empty)
    }

    func testOpeningASetReplacesTheOneBefore() throws {
        let set = try Fixture(sizes: [(64, 64), (64, 64), (64, 64)])
        set.store.openPaths(set.paths)
        XCTAssertEqual(set.store.library.count, 3)

        set.store.openPaths([set.paths[2]])
        XCTAssertNil(set.store.problem)
        XCTAssertEqual(set.store.library.count, 1)
        XCTAssertEqual(set.store.library.entries.first?.path.path, set.paths[2].path)
    }

    // ---- moving between photographs ---------------------------------------

    func testTheEditOnThePhotographLeftBehindComesBack() throws {
        // The whole reason the library holds a history per photograph. Clicking
        // the wrong thumbnail and clicking back must not cost an hour of undo.
        let set = try Fixture(sizes: [(64, 64), (64, 64)])
        set.store.openPaths(set.paths)

        let row = try XCTUnwrap(set.store.addEffect("sharpen"))
        set.store.setFloat(row: row, key: "amount", value: 1.5)

        set.store.focus(1)
        XCTAssertNil(set.store.problem)
        XCTAssertEqual(set.store.library.current, 1)
        XCTAssertFalse(
            set.store.snapshot.rows.contains { $0.effect == "sharpen" },
            "one photograph's edit followed the person to another")
        // The mark the strip draws: the one left behind has something in it to
        // undo, and the one on screen holds its history in hand rather than
        // parked, so it reads untouched until it is switched away from.
        XCTAssertEqual(set.store.library.entries.map(\.edited), [true, false])

        set.store.focus(0)
        XCTAssertEqual(set.store.library.current, 0)
        let amount = try XCTUnwrap(
            set.store.snapshot.rows.first { $0.effect == "sharpen" }?
                .params["amount"]?.floatValue,
            "switching away threw the edit out")
        XCTAssertEqual(amount, 1.5, accuracy: 0.0001)
        XCTAssertTrue(set.store.canUndo, "the undo stack did not come back with it")
    }

    func testFocusPastTheEndIsRefusedAndNothingMoves() throws {
        let set = try Fixture(sizes: [(64, 64), (64, 64)])
        set.store.openPaths(set.paths)
        set.store.focus(1)
        XCTAssertNil(set.store.problem)

        set.store.focus(7)
        XCTAssertNotNil(set.store.problem, "the engine focused a photograph that is not there")
        XCTAssertEqual(set.store.library.current, 1, "the set moved anyway")

        // And with no set at all there is nothing to focus.
        let alone = try XCTUnwrap(SessionStore())
        alone.openTestChart(width: 64, height: 64)
        alone.focus(0)
        XCTAssertNotNil(alone.problem)
    }

    // ---- thumbnails --------------------------------------------------------

    func testAThumbnailArrivesAtTheEnginesSizeAndBecomesAPicture() throws {
        // 128 on the long edge, the short one following the photograph — so
        // neither is worth assuming, and a picture built to a guessed size
        // would be the wrong shape.
        let set = try Fixture(sizes: [(256, 128), (300, 100), (64, 64)])
        set.store.openPaths(set.paths)
        set.store.requestThumbnails(0..<3)
        for index in 0..<3 {
            XCTAssertTrue(waitForThumbnail(set.store, index), "no thumbnail for entry \(index)")
        }

        XCTAssertEqual(set.store.library.entries.filter(\.hasThumbnail).count, 3)
        XCTAssertEqual(set.store.library.entries.filter(\.failed).count, 0)

        let wide = try XCTUnwrap(set.store.thumbnail(at: 0))
        XCTAssertEqual(wide.width, 128)
        XCTAssertEqual(wide.height, 64)

        // 300x100 does not reduce to a whole number of rows, so this is the
        // engine's rounding rather than this side's arithmetic.
        let odd = try XCTUnwrap(set.store.thumbnail(at: 1))
        XCTAssertEqual(odd.width, 128)
        XCTAssertEqual(odd.height, 43)

        // And a photograph smaller than a thumbnail is not blown up to fill one.
        let small = try XCTUnwrap(set.store.thumbnail(at: 2))
        XCTAssertEqual(small.width, 64)
        XCTAssertEqual(small.height, 64)
    }

    func testTheBytesComeAcrossAsTheColoursThatWereWritten() throws {
        // Not a shape check. The ABI's bytes are RGBA, rows top to bottom, and
        // a picture built from them with the channels in another order is a
        // filmstrip of blue photographs that no size assertion would catch.
        let set = try Fixture(sizes: [(256, 128)])
        let session = try XCTUnwrap(Session())
        try session.setSupportDirectory(set.support)
        try session.openPaths(set.paths)
        XCTAssertEqual(session.entryCount, 1)
        XCTAssertEqual(session.currentEntry, 0)
        XCTAssertEqual(session.entryPath(0)?.path, set.paths[0].path)

        // Nothing has been asked for, so there is nothing to read yet — and
        // that is a nil rather than a picture of nothing.
        XCTAssertNil(try session.thumbnail(0))

        session.requestThumbnails(0..<1)
        let deadline = Date().addingTimeInterval(30)
        while Date() < deadline, session.entryFlags(0)?.hasThumbnail != true {
            session.collectThumbnails()
            Thread.sleep(forTimeInterval: 0.005)
        }
        let thumb = try XCTUnwrap(try session.thumbnail(0))
        XCTAssertEqual(thumb.width, 128)
        XCTAssertEqual(thumb.height, 64)
        XCTAssertEqual(thumb.rgba.count, thumb.width * thumb.height * 4)

        // The photograph's top-left quarter is the red block `writePNG` paints,
        // so the first pixel of the first row is red and not grey.
        XCTAssertGreaterThan(thumb.rgba[0], 200, "red did not come across as red")
        XCTAssertLessThan(thumb.rgba[1], 60)
        XCTAssertLessThan(thumb.rgba[2], 60)
        // The fourth byte is padding the engine writes 255 into, which is why
        // the picture reads it as none rather than as coverage.
        XCTAssertEqual(thumb.rgba[3], 255)
        // The far end of the same row is the grey field, which is grey in every
        // channel — so a picture built with the bytes rotated would read red
        // here as well as at the start.
        let last = (thumb.width - 1) * 4
        XCTAssertGreaterThan(thumb.rgba[last], 100)
        XCTAssertLessThan(abs(Int(thumb.rgba[last]) - Int(thumb.rgba[last + 1])), 5)
        XCTAssertLessThan(abs(Int(thumb.rgba[last]) - Int(thumb.rgba[last + 2])), 5)
    }

    func testThePicturesAreBuiltOnceAndNotOnEveryCollect() throws {
        // The point of the whole arrangement. A thumbnail is 64 KB and a set
        // can be two hundred of them, so a store that re-copied whenever it was
        // asked would spend thirteen megabytes to arrive at the pictures it
        // already had.
        let set = try Fixture(sizes: [(256, 128), (256, 128)])
        set.store.openPaths(set.paths)
        set.store.requestThumbnails(0..<2)
        XCTAssertTrue(waitForThumbnail(set.store, 0))
        XCTAssertTrue(waitForThumbnail(set.store, 1))

        let first = try XCTUnwrap(set.store.thumbnail(at: 0))
        let second = try XCTUnwrap(set.store.thumbnail(at: 1))

        // Nothing more can arrive, so nothing more is copied — and what is
        // handed out is the same object, not an equal-looking rebuild.
        for _ in 0..<20 {
            XCTAssertFalse(
                set.store.collectThumbnails(), "the engine reported a delivery twice")
        }
        XCTAssertTrue(set.store.thumbnail(at: 0) === first, "the picture was built again")
        XCTAssertTrue(set.store.thumbnail(at: 1) === second, "the picture was built again")

        // Nor does an edit, which refreshes everything else about the set.
        let row = try XCTUnwrap(set.store.addEffect("sharpen"))
        set.store.setFloat(row: row, key: "amount", value: 0.5)
        XCTAssertTrue(set.store.thumbnail(at: 0) === first, "an edit rebuilt the pictures")

        // Nor does moving to the other photograph, which does move the marks.
        set.store.focus(1)
        XCTAssertEqual(set.store.library.current, 1)
        XCTAssertTrue(set.store.thumbnail(at: 0) === first, "a switch rebuilt the pictures")
        XCTAssertTrue(set.store.thumbnail(at: 1) === second, "a switch rebuilt the pictures")
    }

    func testAskingTwiceForTheSameThumbnailCostsNothing() throws {
        let set = try Fixture(sizes: [(256, 128)])
        set.store.openPaths(set.paths)
        set.store.requestThumbnails(0..<1)
        XCTAssertTrue(waitForThumbnail(set.store, 0))
        let picture = try XCTUnwrap(set.store.thumbnail(at: 0))

        // The second ask is dropped by the engine, so no second decode arrives;
        // a store that saw one would rebuild the picture for nothing. The range
        // running off the end of the set is ignored rather than refused.
        set.store.requestThumbnails(0..<1)
        set.store.requestThumbnails(0..<64)
        for _ in 0..<20 {
            XCTAssertFalse(set.store.collectThumbnails())
        }
        XCTAssertTrue(set.store.thumbnail(at: 0) === picture)
        XCTAssertNil(set.store.problem)
    }

    func testThumbnailsOfNothingAreANoOpRatherThanAFailure() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 64, height: 64)
        store.requestThumbnails(0..<10)
        XCTAssertFalse(store.collectThumbnails())
        XCTAssertNil(store.thumbnail(at: 0))
        XCTAssertNil(store.problem)
    }

    func testThePicturesOfASetThatHasGoneAreDropped() throws {
        // A picture is 64 KB of memory. A session worked through folder after
        // folder must not end up holding one of every photograph it has ever
        // been shown, which is the accounting a filmstrip exists to avoid.
        let set = try Fixture(sizes: [(256, 128), (256, 128)])
        set.store.openPaths(set.paths)
        set.store.requestThumbnails(0..<2)
        XCTAssertTrue(waitForThumbnail(set.store, 0))
        XCTAssertTrue(waitForThumbnail(set.store, 1))
        XCTAssertNotNil(set.store.thumbnail(at: 0))
        XCTAssertNotNil(set.store.thumbnail(at: 1))

        set.store.openPaths([set.paths[1]])
        XCTAssertEqual(set.store.library.count, 1)
        // Nothing is left under an index that now names something else.
        XCTAssertNil(set.store.thumbnail(at: 1))
        // The one photograph still in the set keeps its picture, because the
        // cache is keyed by the path and not by where it happens to sit.
        XCTAssertNotNil(set.store.thumbnail(at: 0))

        set.store.openTestChart(width: 64, height: 64)
        XCTAssertEqual(set.store.library, .empty)
        XCTAssertNil(set.store.thumbnail(at: 0))
    }

    // ---- the picture itself ------------------------------------------------

    func testBytesThatAreNotTheShapeTheyClaimAreNotAPicture() {
        // Nothing in the ABI can produce this, and that is the point: if it
        // ever did, the answer is no picture rather than a Core Graphics image
        // reading off the end of a buffer.
        XCTAssertNil(Thumbnail(width: 4, height: 4, rgba: [0, 0, 0, 255]).image)
        XCTAssertNil(Thumbnail(width: 0, height: 0, rgba: []).image)

        let whole = Thumbnail(
            width: 2, height: 2, rgba: [UInt8](repeating: 200, count: 2 * 2 * 4))
        let picture = whole.image
        XCTAssertEqual(picture?.width, 2)
        XCTAssertEqual(picture?.height, 2)
    }

    // ---- the fixtures ------------------------------------------------------

    /// A temporary directory of real photographs, and a store pointed at a
    /// support directory inside it so that no test writes an autosave into the
    /// person's own Application Support.
    @MainActor
    private struct Fixture {
        let directory: URL
        let support: URL
        let paths: [URL]
        let store: SessionStore

        init(sizes: [(Int, Int)]) throws {
            directory = URL(fileURLWithPath: NSTemporaryDirectory())
                .appendingPathComponent(UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(
                at: directory, withIntermediateDirectories: true)
            support = directory.appendingPathComponent("support", isDirectory: true)

            var written: [URL] = []
            for (i, size) in sizes.enumerated() {
                let name = String(UnicodeScalar(UInt8(97 + i))) + ".png"
                let url = directory.appendingPathComponent(name)
                try LibraryTests.writePNG(url, width: size.0, height: size.1)
                written.append(url)
            }
            paths = written

            guard let store = SessionStore() else {
                throw Failure(what: "the engine would not start")
            }
            store.setSupportDirectory(support)
            self.store = store
        }
    }

    private struct Failure: Error, CustomStringConvertible {
        let what: String
        var description: String { what }
    }

    /// Write a real PNG: a red block in one corner over a grey field, so that a
    /// thumbnail of it is a picture of something rather than a flat colour that
    /// would read the same however the channels were ordered.
    ///
    /// The bundle's one PNG writer — `BatchTests` needs real photographs on
    /// disc for the same reason this does, and a second copy of an encoder is a
    /// second copy to drift.
    static func writePNG(_ url: URL, width: Int, height: Int) throws {
        guard
            let context = CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
        else { throw Failure(what: "no bitmap context for \(width)x\(height)") }

        context.setFillColor(red: 0.5, green: 0.5, blue: 0.5, alpha: 1)
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        context.setFillColor(red: 1, green: 0, blue: 0, alpha: 1)
        // Core Graphics puts the origin at the bottom left and a thumbnail's
        // rows run top to bottom, so this rectangle is the photograph's *top*
        // left quarter and therefore its first pixel.
        context.fill(
            CGRect(
                x: 0, y: height - height / 2,
                width: max(width / 2, 1), height: max(height / 2, 1)))

        guard let image = context.makeImage() else {
            throw Failure(what: "the fixture would not become an image")
        }
        guard
            let destination = CGImageDestinationCreateWithURL(
                url as CFURL, UTType.png.identifier as CFString, 1, nil)
        else { throw Failure(what: "no PNG encoder for \(url.path)") }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else {
            throw Failure(what: "the fixture was not written to \(url.path)")
        }
    }

    /// Poll until an entry's thumbnail has arrived, or the worker has given up
    /// on it.
    ///
    /// The decode is on a real thread, so this waits to a deadline rather than
    /// sleeping for a guessed interval and hoping — a test patient enough to be
    /// certain is slow for everybody, and one that is not is fine until it runs
    /// on a loaded machine.
    private func waitForThumbnail(_ store: SessionStore, _ index: Int) -> Bool {
        let deadline = Date().addingTimeInterval(30)
        while Date() < deadline {
            store.collectThumbnails()
            if let entry = store.library[index], entry.hasThumbnail || entry.failed {
                XCTAssertFalse(entry.failed, "the worker could not read a file just written")
                return entry.hasThumbnail
            }
            Thread.sleep(forTimeInterval: 0.005)
        }
        return false
    }
}
