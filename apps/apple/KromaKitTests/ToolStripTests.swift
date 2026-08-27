import AppKit
import XCTest
// Same module as the code under test; see EngineTests.swift.

/// The strip, and what it promises.
///
/// Two of these are about a thing the compiler cannot see. A tool's symbol is
/// a *string* handed to the system, so a name the system does not have is not
/// a build error — it is a button that draws nothing, in a strip of eight,
/// with no way for anybody to tell which one it was. And which effects a tool
/// claims is written in the engine and again in Swift, so the fixture is what
/// keeps the two lists one list.
final class ToolStripTests: XCTestCase {
    /// The generated theme fixture, which carries the engine's own tool list.
    /// Each test file carries its own copy of this helper so it can be read
    /// alone.
    private func fixture() throws -> [String: Any] {
        let url = try XCTUnwrap(
            Bundle(for: Self.self).url(forResource: "theme", withExtension: "json"),
            "theme.json is not in the test bundle"
        )
        return try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
        )
    }

    private func fixture(_ name: String) throws -> Data {
        let url = try XCTUnwrap(
            Bundle(for: Self.self).url(forResource: name, withExtension: "json"),
            "\(name).json is not in the test bundle"
        )
        return try Data(contentsOf: url)
    }

    // ---- the symbols ------------------------------------------------------

    /// A symbol that does not exist renders as nothing, and a blank button in a
    /// strip of eight is one nobody can identify. `NSImage` returns nil for a
    /// name the system does not have, so this is checkable rather than a hope.
    func testEveryToolHasASymbolTheSystemActuallyHas() {
        for tool in Tool.allCases {
            XCTAssertNotNil(
                NSImage(systemSymbolName: tool.symbol, accessibilityDescription: nil),
                "\(tool.name) asks for \(tool.symbol), which this system does not have")
        }
    }

    /// And no two tools wear the same one, which would be eight buttons and seven
    /// distinguishable ones.
    func testNoTwoToolsShareASymbol() {
        XCTAssertEqual(
            Set(Tool.allCases.map(\.symbol)).count, Tool.allCases.count,
            "two tools draw the same glyph")
    }

    // ---- the grouping -----------------------------------------------------

    func testTheToolsAndTheirEffectsMatchTheEngine() throws {
        let tools = try XCTUnwrap(fixture()["tools"] as? [[String: Any]])
        XCTAssertEqual(tools.count, Tool.allCases.count)
        for (i, entry) in tools.enumerated() {
            let tool = Tool.allCases[i]
            XCTAssertEqual(entry["name"] as? String, tool.name, "tool \(i)")
            XCTAssertEqual(entry["effects"] as? [String], tool.effects, tool.name)
        }
    }

    /// Every pinned effect the registry declares is reachable from some tool.
    /// One that is not would simply not be drawn anywhere, with nothing to say
    /// so — the worst kind of missing control.
    func testEveryPinnedEffectIsReachable() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        let owned = Set(Tool.allCases.flatMap(\.effects))
        for row in snap.rows where row.pinned {
            XCTAssertTrue(
                owned.contains(row.effect),
                "\(row.effect) is pinned but no tool draws it")
        }
    }

    /// The registry's own pinned list, not just the ones this snapshot happens
    /// to carry — the snapshot is one document and the registry is the promise
    /// every document is opened against.
    func testEveryPinnedEffectInTheRegistryIsReachable() throws {
        let registry = try JSONDecoder().decode(Registry.self, from: fixture("registry"))
        XCTAssertFalse(registry.pinned.isEmpty, "the fixture pins nothing")
        let owned = Tool.allCases.flatMap(\.effects)
        XCTAssertEqual(
            Set(owned).count, owned.count, "a tool claims an effect another tool also claims")
        for key in registry.pinned {
            XCTAssertTrue(owned.contains(key), "\(key) is pinned but no tool draws it")
        }
        // And nothing claimed that the registry does not pin: a fixed panel for
        // a row no document starts with is a panel with nothing behind it.
        for key in owned {
            XCTAssertTrue(
                registry.pinned.contains(key), "a tool claims \(key), which is not pinned")
        }
    }

    /// ``Tool/of(_:)`` answers about an effect *key*, not about a row. An
    /// effect nobody pinned has no fixed panel, and saying so is what lets the
    /// inspector draw it as a stack row rather than hunt for a tool that does
    /// not exist.
    ///
    /// A row is a different question: the fixture's added row is a *second*
    /// exposure, whose key is pinned and whose row is not, which is why the
    /// inspector asks `row.pinned` first and this method second.
    func testAnEffectNobodyPinnedHasNoTool() {
        XCTAssertEqual(Tool.of("colour_warper"), .colourWarper)
        XCTAssertNil(Tool.of("dehaze"))
        XCTAssertNil(Tool.of("not_an_effect"))
    }

    /// And the added row really is one the pinned panels must not pick up:
    /// two rows carry the same key, and only the pinned one is a fixed panel.
    func testAPinnedPanelTakesThePinnedRowAndNotAnAddedCopyOfIt() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        let added = try XCTUnwrap(snap.rows.first { !$0.pinned })
        XCTAssertTrue(
            Tool.basic.effects.contains(added.effect),
            "the fixture no longer adds a second copy of a pinned effect, so this "
                + "checks nothing")
        let panel = try XCTUnwrap(
            snap.rows.first { $0.pinned && $0.effect == added.effect })
        XCTAssertNotEqual(panel.id, added.id)
    }

    /// Nothing in a document goes undrawn, and nothing is drawn twice.
    ///
    /// With one tool on screen, a row no tool claims is a control that exists,
    /// responds to nothing and appears nowhere — and there is no scroll far
    /// enough to find it. So the tools are checked as a *partition* of the
    /// document rather than one at a time.
    func testTheToolsBetweenThemDrawEveryRowExactlyOnce() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        XCTAssertFalse(snap.rows.isEmpty, "the fixture document has no rows")
        XCTAssertEqual(
            Tool.allCases.flatMap { $0.draws(snap.rows) }.map(\.id).sorted(),
            snap.rows.map(\.id).sorted())
    }

    /// And a tool draws each of its rows at the row's own place in the stack,
    /// which is what the reorder arrows move by.
    func testARowKeepsItsPlaceInTheWholeStack() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        for drawn in Tool.allCases.flatMap({ $0.draws(snap.rows) }) {
            XCTAssertEqual(snap.rows[drawn.index].id, drawn.row.id)
        }
    }

    /// Exactly three tools have no pinned effects of their own, and each is
    /// deliberate: Effects shows whatever the user put there and is where the
    /// browser lives, Crop edits the document's geometry rather than a row in
    /// its stack, and File is about the file rather than the picture.
    ///
    /// Named rather than counted, and the mirror of
    /// `only_effects_crop_and_file_own_nothing_pinned` in `pe-effects`. What it
    /// catches is a *fourth* — a tool added to the strip and its effects list
    /// forgotten, which draws an empty panel and takes its effects off screen
    /// with it.
    func testOnlyEffectsCropAndFileOwnNothingPinned() {
        XCTAssertEqual(
            Tool.allCases.filter { $0.effects.isEmpty }, [.effects, .crop, .file],
            "a tool with no pinned effects that is not one of these three is a panel "
                + "that draws nothing")
    }

    /// And Crop is the one tool the viewer changes for.
    ///
    /// `showsWholeFrame` is read twice in `ContentView` — once to put the
    /// overlay over the picture and once to tell the engine to frame the
    /// enclosing rectangle — so that the two cannot disagree. Left on after
    /// switching away, every other tool would be graded against a picture with
    /// the cut-away parts still in it.
    func testOnlyCropFramesTheWholeSource() {
        XCTAssertEqual(Tool.allCases.filter(\.showsWholeFrame), [.crop])
    }

    // ---- the strip --------------------------------------------------------

    func testTheStripOpensOnBasic() {
        XCTAssertEqual(Tool.allCases.first, .basic)
    }

    /// Each tool names what it will show, so a strip button is not a guess.
    func testEveryToolHasAnAccessibilityLabel() {
        for tool in Tool.allCases {
            XCTAssertFalse(tool.name.isEmpty)
        }
    }

    /// What is remembered between launches is the raw value, so it has to read
    /// back as the tool it was written for. A renamed case would otherwise
    /// silently open on Basic every time.
    func testAToolReadsBackAsWhatWasStored() {
        for tool in Tool.allCases {
            XCTAssertEqual(Tool(rawValue: tool.rawValue), tool)
        }
        XCTAssertNil(Tool(rawValue: "Colour Warper "), "the stored name is exact")
    }

    // ---- and against a live engine ----------------------------------------

    /// Adding an effect still works, and the row it adds appears where the
    /// reader is looking.
    ///
    /// The browser used to sit under every panel; it belongs to `Effects` now,
    /// which means the row it adds and the button that added it are the same
    /// screen. Asked of a real session rather than of a fixture, because "the
    /// engine accepted it" and "some tool draws it" are the two halves of the
    /// claim and only the first of them is about JSON.
    @MainActor
    func testAddingAnEffectPutsARowUnderTheToolThatAddedIt() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()

        let before = Tool.effects.draws(store.snapshot.rows)
        XCTAssertTrue(before.isEmpty, "a fresh document has nothing added yet")

        let id = try XCTUnwrap(store.addEffect("sharpen"), store.problem ?? "refused")
        let after = Tool.effects.draws(store.snapshot.rows)
        XCTAssertEqual(after.map(\.id), [id], "the added row is not under Effects")
        XCTAssertEqual(after.first?.row.effect, "sharpen")

        // Still nothing lost and nothing doubled, now that the document has an
        // added row in it as well as the eleven pinned ones.
        XCTAssertEqual(
            Tool.allCases.flatMap { $0.draws(store.snapshot.rows) }.map(\.id).sorted(),
            store.snapshot.rows.map(\.id).sorted())
    }

    /// Every pinned panel of a real document lands under some tool, and each
    /// tool's own strip order is what it draws in.
    @MainActor
    func testARealDocumentsPinnedPanelsAreAllReachable() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        XCTAssertFalse(store.snapshot.rows.isEmpty, "the chart did not open")

        for tool in Tool.allCases where tool != .effects {
            XCTAssertEqual(
                tool.draws(store.snapshot.rows).map(\.row.effect), tool.effects,
                "\(tool.name) does not draw what it claims")
        }
        // Including Crop, which claims nothing and so must draw nothing: its
        // panel is the document's geometry, not a row of its stack.
        XCTAssertTrue(Tool.crop.draws(store.snapshot.rows).isEmpty)
    }
}
