import XCTest
// Same module as the code under test; see EngineTests.swift.

/// The grade in hand: copy a look off one photograph and put it on another.
///
/// Driven through a real engine rather than a fixture, because the whole
/// question is what the *session* does — the clipboard lives there, on purpose,
/// so that two shells cannot end up with two clipboards.
@MainActor
final class GradeTests: XCTestCase {
    private func opened() throws -> SessionStore {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        XCTAssertFalse(store.snapshot.rows.isEmpty, "the chart did not open")
        return store
    }

    /// Nothing is copied until something is copied, which is what the Paste
    /// items are greyed by.
    func testNothingIsInHandToBeginWith() throws {
        let store = try opened()
        XCTAssertFalse(store.hasGrade)
    }

    /// `hasGrade` must be a *stored* property, because that is what SwiftUI's
    /// `@Observable` tracks. Computed, and reaching through to the engine, it
    /// was invisible to it: the Paste menu item stayed grey after a copy, with
    /// "grade copied" written in the status bar beside it.
    ///
    /// A test cannot watch a menu redraw, so it checks the property that makes
    /// the redraw possible. A stored property appears among a `Mirror`'s
    /// children; a computed one does not — which is the difference the bug was.
    func testHasGradeIsStoredSoThatItCanBeObserved() throws {
        let store = try opened()
        let stored = Mirror(reflecting: store).children.map(\.label)
        XCTAssertTrue(
            stored.contains("_hasGrade") || stored.contains("hasGrade"),
            "hasGrade is computed, so @Observable cannot see it change and the "
                + "Paste items keep whatever state they were built with")
    }

    /// And it follows the copy, which is the behaviour the menu is bound to.
    func testHasGradeFollowsTheCopy() throws {
        let store = try opened()
        XCTAssertFalse(store.hasGrade)
        store.copyGrade()
        XCTAssertTrue(store.hasGrade)
    }

    func testCopyingPutsAGradeInHand() throws {
        let store = try opened()
        store.copyGrade()
        XCTAssertTrue(store.hasGrade)
        XCTAssertNil(store.problem)
        XCTAssertEqual(store.notice, "grade copied")
    }

    /// Pasting replaces the stack and is one undo step. The added row is what
    /// makes the two stacks differ, so it is what proves the replacement.
    func testPastingReplacesTheStackInOneStep() throws {
        let store = try opened()
        store.copyGrade()
        let before = store.snapshot.rows.count

        XCTAssertNotNil(store.addEffect("sharpen"), store.problem ?? "refused")
        XCTAssertEqual(store.snapshot.rows.count, before + 1)

        store.pasteGrade()
        XCTAssertNil(store.problem)
        XCTAssertEqual(store.snapshot.rows.count, before, "the paste did not replace")

        store.undo()
        XCTAssertEqual(
            store.snapshot.rows.count, before + 1, "one undo did not put the row back")
    }

    /// A paste says nothing out loud, and deliberately: the picture changed,
    /// and that is the notice.
    func testPastingSaysNothing() throws {
        let store = try opened()
        store.copyGrade()
        store.pasteGrade()
        XCTAssertNil(store.problem)
        XCTAssertNil(store.notice, "a visible change does not need saying as well")
    }

    /// Pasting with nothing in hand is refused, and the refusal goes to
    /// `problem` — not to `notice`, which would draw a failure as a success.
    func testPastingWithNothingCopiedIsARefusalAndNotANotice() throws {
        let store = try opened()
        store.pasteGrade()
        let problem = try XCTUnwrap(store.problem, "the engine accepted a paste of nothing")
        XCTAssertTrue(
            problem.contains("no grade has been copied"),
            "the refusal does not say which of the two it was: \(problem)")
        XCTAssertNil(store.notice)
    }

    /// And a refusal clears whatever succeeded before it. Two messages from two
    /// different commands, one of them stale, is worse than one.
    func testARefusalClearsTheLastNotice() throws {
        let store = try opened()
        store.copyGrade()
        XCTAssertEqual(store.notice, "grade copied")

        // Nothing else is open, so there is nothing for this to paste onto.
        store.pasteGradeToAll()
        XCTAssertNotNil(store.problem)
        XCTAssertNil(store.notice, "the copy's notice outlived the failure after it")
    }

    /// Pasting to a set that is not open is refused rather than reported as
    /// having reached nobody: "pasted to 0 photos" reads as success.
    func testPastingToAllWithNoSetOpenIsRefused() throws {
        let store = try opened()
        store.copyGrade()
        store.pasteGradeToAll()
        XCTAssertNotNil(store.problem, "a set of one accepted a paste to all")
    }
}
