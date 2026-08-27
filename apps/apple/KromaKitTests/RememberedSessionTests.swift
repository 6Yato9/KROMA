import XCTest

// Same module as the code under test; see EngineTests.swift.

/// What the application remembers between runs, through the store the
/// interface reads: the set that was open, and the stars.
///
/// Against a real engine and real files on disc, because the whole subject is
/// files: which of them are still there, which of them still decode, and what a
/// launch does about the ones that do not. A stub would assert this side's
/// opinion of all three.
///
/// **The one that earns its place is
/// ``testAPhotographThatWillNotDecodeDoesNotCostTheLaunch``.** The engine drops
/// the photographs that are *gone* — `is_file` is as far as it can go without
/// decoding every frame of a folder at start-up — so a file that is still there
/// and will not read comes back in the remembered set and is a refusal on the
/// way in. A launch that died on that refusal would be a window that never
/// appears, with no way out but finding and deleting a settings file. This is
/// the test that says it does not.
@MainActor
final class RememberedSessionTests: XCTestCase {

    /// Nothing remembered is the built-in chart, which is what a first run has
    /// always done and is also what a folder that has been tidied away leaves
    /// behind.
    func testAFirstRunOpensTheChart() throws {
        let shoot = try Shoot(count: 3)
        let store = try shoot.store()

        store.openRemembered()
        XCTAssertTrue(store.snapshot.isOpen, "nothing opened at all")
        XCTAssertTrue(store.library.isEmpty, "a first run opened somebody's photographs")
        XCTAssertNil(store.problem)
    }

    /// The set comes back, and so does which one was showing. Remembering the
    /// photographs and forgetting the place in them puts you back at the front
    /// of a folder of two hundred.
    func testTheSetThatWasOpenComesBackWithTheOneThatWasShowing() throws {
        let shoot = try Shoot(count: 3)
        let first = try shoot.store()
        first.openPaths(shoot.paths)
        first.focus(2)
        XCTAssertNil(first.problem)

        let next = try shoot.store()
        next.openRemembered()
        XCTAssertNil(next.problem)
        XCTAssertEqual(next.library.entries.map { $0.path.path }, shoot.paths.map(\.path))
        XCTAssertEqual(next.library.current, 2, "reopened on the wrong photograph")
        XCTAssertEqual(next.snapshot.name, "c.png")
    }

    /// One photograph moved or deleted must not stop the others opening, and
    /// must not slide the answer onto its neighbour: the one that was showing
    /// is remembered by name, so losing one from the front of the set does not
    /// renumber it onto the wrong picture.
    func testAPhotographThatHasGoneDoesNotStopTheOthersOpening() throws {
        let shoot = try Shoot(count: 3)
        let first = try shoot.store()
        first.openPaths(shoot.paths)
        first.focus(2)

        try FileManager.default.removeItem(at: shoot.paths[1])

        let next = try shoot.store()
        next.openRemembered()
        XCTAssertNil(next.problem, "a deleted photograph was reported as a failure")
        XCTAssertEqual(next.library.entries.map(\.name), ["a.png", "c.png"])
        XCTAssertEqual(next.snapshot.name, "c.png", "the deletion moved which one reopened")
    }

    /// A photograph that is still there and will not decode is the case the
    /// engine cannot answer, and this is the Mac's answer: the one that refuses
    /// is dropped, the rest of the set opens, and the reason goes to the status
    /// bar — where there is now a window to say it in.
    func testAPhotographThatWillNotDecodeDoesNotCostTheLaunch() throws {
        let shoot = try Shoot(count: 3)
        let first = try shoot.store()
        first.openPaths(shoot.paths)

        // Still there, still named the same, and no longer a photograph. This
        // is a truncated download, an interrupted card copy, a file the
        // operating system has not finished writing.
        try Data("not a PNG any more".utf8).write(to: shoot.paths[0])

        let next = try shoot.store()
        next.openRemembered()
        XCTAssertTrue(next.snapshot.isOpen, "one bad photograph cost the whole launch")
        XCTAssertEqual(
            next.library.entries.map(\.name), ["b.png", "c.png"],
            "the set that opened is not the rest of the set")
        XCTAssertEqual(next.snapshot.name, "b.png")
        let said = try XCTUnwrap(
            next.problem, "a photograph would not open and nobody was told")
        XCTAssertTrue(
            said.contains("a.png"), "the refusal does not name the photograph: \(said)")
    }

    /// And when the one that was showing is the one that will not decode: the
    /// set still opens, on the first photograph rather than on nothing, and
    /// says what happened.
    func testTheOneThatWasShowingWillNotDecodeSoTheSetOpensWhereItCan() throws {
        let shoot = try Shoot(count: 3)
        let first = try shoot.store()
        first.openPaths(shoot.paths)
        first.focus(2)

        try Data("not a PNG any more".utf8).write(to: shoot.paths[2])

        let next = try shoot.store()
        next.openRemembered()
        XCTAssertTrue(next.snapshot.isOpen)
        // The whole set, because the one that refused is not the one that had
        // to be decoded to open it.
        XCTAssertEqual(next.library.entries.map(\.name), ["a.png", "b.png", "c.png"])
        XCTAssertEqual(next.library.current, 0, "it opened on a photograph that will not read")
        let said = try XCTUnwrap(next.problem, "the focus was refused and nobody was told")
        XCTAssertTrue(said.contains("c.png"), "the refusal does not name it: \(said)")
    }

    /// Nothing in the set will open. The chart is the fallback rather than a
    /// window that never appears — and the first refusal, which is the one that
    /// names the photograph the person was actually on, is still said.
    func testASetWhereNothingWillDecodeFallsBackToTheChart() throws {
        let shoot = try Shoot(count: 2)
        let first = try shoot.store()
        first.openPaths(shoot.paths)
        for path in shoot.paths {
            try Data("not a PNG any more".utf8).write(to: path)
        }

        let next = try shoot.store()
        next.openRemembered()
        XCTAssertTrue(next.snapshot.isOpen, "nothing opened at all")
        XCTAssertTrue(next.library.isEmpty, "a set of unreadable photographs opened")
        let said = try XCTUnwrap(next.problem, "the launch fell back and nobody was told")
        XCTAssertTrue(said.contains("a.png"), "the first refusal was not the one kept: \(said)")
    }

    // ---- the stars ---------------------------------------------------------

    /// A star that vanishes when the window closes is half a feature, which is
    /// the whole reason any of this is in the engine rather than in
    /// `@AppStorage`.
    func testAStarSurvivesTheWindowClosing() throws {
        let shoot = try Shoot(count: 1)
        let first = try shoot.store()
        XCTAssertFalse(first.isFavourite("grain"), "something is starred to begin with")
        XCTAssertEqual(first.favourites, [])

        first.toggleFavourite("grain")
        XCTAssertTrue(first.isFavourite("grain"))
        XCTAssertEqual(first.favourites, ["grain"])
        XCTAssertNil(first.problem)

        let next = try shoot.store()
        XCTAssertTrue(
            next.isFavourite("grain"), "the star did not survive being written and read back")
        XCTAssertEqual(next.favourites, ["grain"])

        // The same gesture takes it off again, and that survives too.
        next.toggleFavourite("grain")
        XCTAssertEqual(next.favourites, [])
        let third = try shoot.store()
        XCTAssertFalse(third.isFavourite("grain"), "unstarring did not survive")
    }

    /// A store that has not been told where its support directory is has not
    /// read a settings file, and must not pretend it has.
    func testAStoreWithNoSupportDirectoryStartsWithNothingStarred() throws {
        let store = try XCTUnwrap(SessionStore())
        XCTAssertEqual(store.favourites, [])
        XCTAssertFalse(store.isFavourite("grain"))
    }

    // ---- the fixtures ------------------------------------------------------

    /// A folder of real photographs and a support directory beside them, so
    /// that no test writes into the person's own Application Support — and so
    /// that every store built from it is a *later launch* of the same
    /// application.
    @MainActor
    private struct Shoot {
        let directory: URL
        let support: URL
        let paths: [URL]

        init(count: Int) throws {
            directory = URL(fileURLWithPath: NSTemporaryDirectory())
                .appendingPathComponent(UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(
                at: directory, withIntermediateDirectories: true)
            support = directory.appendingPathComponent("support", isDirectory: true)

            var written: [URL] = []
            for i in 0..<count {
                let name = String(UnicodeScalar(UInt8(97 + i))) + ".png"
                let url = directory.appendingPathComponent(name)
                // The bundle's one PNG writer, in `LibraryTests`. A second copy
                // of an encoder is a second copy to drift.
                try LibraryTests.writePNG(url, width: 64, height: 64)
                written.append(url)
            }
            paths = written
        }

        /// The next launch: a new session, told where the same settings file
        /// is.
        func store() throws -> SessionStore {
            guard let store = SessionStore() else {
                throw Failure(what: "the engine would not start")
            }
            store.setSupportDirectory(support)
            return store
        }
    }

    private struct Failure: Error, CustomStringConvertible {
        let what: String
        var description: String { what }
    }
}
