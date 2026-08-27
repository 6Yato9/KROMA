import XCTest

// Same module as the code under test; see EngineTests.swift.

/// A batch export, through the store the interface reads.
///
/// **On disc**, because that is the only thing that proves a batch ran: a
/// return code says a step happened, not that a file arrived. Everything here
/// is checkable without a screen, which is the point — the run is a step a
/// frame and three counts, and the summary at the end is a string.
///
/// Real files and a real engine, the way `LibraryTests` does it, because every
/// interesting thing here is the engine's: the decode per photograph, the
/// refusal to write over somebody's original, the counts a finished run keeps
/// until they are read.
@MainActor
final class BatchTests: XCTestCase {

    // ---- what a run does ---------------------------------------------------

    func testAStepAFrameWritesOneFilePerPhotographAndSaysSoAtTheEnd() throws {
        let set = try Fixture(count: 3)
        XCTAssertTrue(set.store.canStartBatch)

        set.store.startBatch(into: set.out)
        XCTAssertNil(set.store.problem)
        XCTAssertEqual(
            set.store.batch, BatchCounts(done: 0, failed: 0, total: 3),
            "the run did not report itself before its first step")
        XCTAssertFalse(
            set.store.canStartBatch, "a second run could be started over the first")

        // One step a frame, and the loop ends on the store's own answer rather
        // than on a count known in advance.
        let steps = set.run()
        XCTAssertEqual(steps, 3, "one step per photograph, no more")

        for name in ["a_KROMA.png", "b_KROMA.png", "c_KROMA.png"] {
            XCTAssertTrue(set.wrote(name), "\(name) was not written")
        }

        // Read *after* the step that answered "no more", which is only possible
        // because a finished run keeps its counts until it is put away. A run
        // that stopped without a word is indistinguishable from one that
        // crashed on its first photograph.
        XCTAssertEqual(set.store.batchSummary, "3 exported")
        XCTAssertNil(set.store.batch, "a finished run is still drawing a progress bar")
        XCTAssertTrue(set.store.canStartBatch)

        // And a step afterwards does nothing rather than starting it again.
        XCTAssertFalse(set.store.stepBatch())
        XCTAssertEqual(set.store.batchSummary, "3 exported")
    }

    /// The run is stepped from the frame tick — the same place `renderIfNeeded`
    /// collects thumbnails and drives the autosave — and not from a `body`, and
    /// not in a loop on the button. A full-resolution render inside a view
    /// update is a frozen window with extra steps.
    ///
    /// So this drives nothing but the tick, and one photograph comes out per
    /// frame.
    func testTheFrameTickIsWhatStepsARun() throws {
        let set = try Fixture(count: 2)
        set.store.startBatch(into: set.out)
        XCTAssertNil(set.store.problem)

        // Bounded, so that a tick which never steps is a failure rather than a
        // test that hangs: `XCTAssertLessThan` records and carries on, it does
        // not end a loop.
        var frames = 0
        while set.store.batch != nil, frames < 64 {
            set.store.renderIfNeeded()
            frames += 1
        }

        XCTAssertNil(set.store.batch, "the frame tick never stepped the run")
        XCTAssertEqual(frames, 2, "a frame exported more than one photograph")
        XCTAssertTrue(set.wrote("a_KROMA.png"))
        XCTAssertTrue(set.wrote("b_KROMA.png"))
        XCTAssertEqual(set.store.batchSummary, "2 exported")
    }

    // ---- what does not stop it ---------------------------------------------

    func testAPhotographThatCannotBeWrittenIsCountedAndSteppedPast() throws {
        // Contrived deliberately, and not far-fetched: a folder exported once
        // already, exported into again. `sunset.png` would land on the
        // `sunset_KROMA.png` sitting beside it, which is somebody's file.
        let set = try Fixture(names: ["sunset.png", "sunset_KROMA.png"])
        let already = set.directory.appendingPathComponent("sunset_KROMA.png")
        let untouched = try Data(contentsOf: already)

        set.store.startBatch(into: set.directory)
        XCTAssertNil(set.store.problem)
        XCTAssertEqual(set.run(), 2, "the collision abandoned the run")

        XCTAssertEqual(
            try Data(contentsOf: already), untouched, "an original was written over")
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: set.directory.appendingPathComponent("sunset_KROMA_KROMA.png").path),
            "one collision abandoned the photograph after it")
        // Counted, and said out loud: sixty-five exported and one missed is not
        // the same run as sixty-six exported.
        XCTAssertEqual(set.store.batchSummary, "1 exported, 1 failed")
    }

    // ---- stopping ----------------------------------------------------------

    func testStoppingKeepsWhatWasWrittenAndSaysWhereItGotTo() throws {
        let set = try Fixture(count: 3)
        set.store.startBatch(into: set.out)

        XCTAssertTrue(set.store.stepBatch())
        XCTAssertEqual(set.store.batch, BatchCounts(done: 1, failed: 0, total: 3))

        set.store.cancelBatch()
        XCTAssertNil(set.store.batch, "a stopped run is still a run")
        XCTAssertEqual(set.store.batchSummary, "stopped after 1 exported")

        // Nothing is taken back: half a folder of exports is the state somebody
        // asked for when they pressed stop.
        XCTAssertTrue(set.wrote("a_KROMA.png"), "stopping took back what was written")
        XCTAssertFalse(set.wrote("b_KROMA.png"), "the run carried on past the stop")

        // And the frame tick afterwards does not restart it.
        set.store.renderIfNeeded()
        XCTAssertFalse(set.wrote("b_KROMA.png"), "a frame after the stop restarted the run")
        XCTAssertNil(set.store.batch)

        set.store.dismissBatchSummary()
        XCTAssertNil(set.store.batchSummary)
    }

    // ---- what will not start -----------------------------------------------

    func testARunWithNoSetIsRefusedRatherThanReportingNoughtFiles() throws {
        let set = try Fixture(count: 1)
        // The built-in chart is not a set of one: there is no file for a run to
        // be a run over.
        set.store.openTestChart(width: 64, height: 64)
        XCTAssertFalse(set.store.canStartBatch)

        set.store.startBatch(into: set.out)
        XCTAssertNotNil(set.store.problem, "the engine ran a batch over no photographs")
        XCTAssertNil(set.store.batch)
        XCTAssertNil(set.store.batchSummary, "a run that never started reported a summary")
        XCTAssertFalse(set.store.stepBatch())
        XCTAssertEqual(
            try FileManager.default.contentsOfDirectory(atPath: set.out.path), [],
            "a refused run wrote something")
    }

    func testASecondRunReplacesTheFirstSummaryAndAll() throws {
        let set = try Fixture(count: 2)
        set.store.startBatch(into: set.out)
        XCTAssertEqual(set.run(), 2)
        XCTAssertEqual(set.store.batchSummary, "2 exported")

        // The same folder again. Each run's claimed names are its own, so this
        // one writes over its own earlier exports — which are nobody's
        // originals — and the summary that comes back is this run's rather than
        // a stale copy of the one before it.
        set.store.startBatch(into: set.out)
        XCTAssertNil(
            set.store.batchSummary, "the run before it was still being reported")
        XCTAssertEqual(set.store.batch, BatchCounts(done: 0, failed: 0, total: 2))
        XCTAssertEqual(set.run(), 2)
        XCTAssertEqual(set.store.batchSummary, "2 exported")
    }

    // ---- the fixtures ------------------------------------------------------

    /// A temporary directory of real photographs, an empty folder to export
    /// into, and a store with the set already open.
    @MainActor
    private struct Fixture {
        let directory: URL
        /// Where a run writes. A folder chosen rather than beside each
        /// original, which is what the engine is for.
        let out: URL
        let paths: [URL]
        let store: SessionStore

        init(names: [String], size: Int = 64) throws {
            directory = URL(fileURLWithPath: NSTemporaryDirectory())
                .appendingPathComponent(UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(
                at: directory, withIntermediateDirectories: true)
            out = directory.appendingPathComponent("out", isDirectory: true)
            try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

            var written: [URL] = []
            for name in names {
                let url = directory.appendingPathComponent(name)
                try LibraryTests.writePNG(url, width: size, height: size)
                written.append(url)
            }
            paths = written

            guard let store = SessionStore() else {
                throw Failure(what: "the engine would not start")
            }
            // Inside the temporary directory, so that no test writes an
            // autosave into the person's own Application Support.
            store.setSupportDirectory(
                directory.appendingPathComponent("support", isDirectory: true))
            store.openPaths(written)
            // PNG rather than the default JPEG, so an exported name is the
            // source's name with a suffix and the assertions can be exact.
            store.setExport(format: "png", quality: 95)
            self.store = store
        }

        init(count: Int) throws {
            try self.init(
                names: (0..<count).map { String(UnicodeScalar(UInt8(97 + $0))) + ".png" })
        }

        /// Step to the end the way the frame loop does, and say how many steps
        /// it took. The condition is the store's own answer, so a run that
        /// stopped early ends this loop rather than hanging it.
        func run(limit: Int = 64) -> Int {
            var steps = 0
            while store.batch != nil, steps < limit {
                XCTAssertTrue(store.stepBatch(), "a run in progress refused to step")
                steps += 1
            }
            XCTAssertLessThan(steps, limit, "a batch that will not finish")
            return steps
        }

        func wrote(_ name: String) -> Bool {
            FileManager.default.fileExists(atPath: out.appendingPathComponent(name).path)
        }
    }

    private struct Failure: Error, CustomStringConvertible {
        let what: String
        var description: String { what }
    }
}
