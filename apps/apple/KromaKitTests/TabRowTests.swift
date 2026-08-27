import AppKit
import XCTest
// Same module as the code under test; see EngineTests.swift.

/// The tab row, and what it promises.
///
/// Two of these are about a thing the compiler cannot see. A tab's symbol is a
/// *string* handed to the system, so a name the system does not have is not a
/// build error — it is a tab that draws no glyph, with no way for anybody to
/// tell which one it was. And which effects a section claims is written in the
/// engine and again in Swift, so the fixture is what keeps the two lists one
/// list.
final class TabRowTests: XCTestCase {
    /// The generated theme fixture, which carries the engine's own tab and
    /// section lists. Each test file carries its own copy of this helper so it
    /// can be read alone.
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

    /// A symbol that does not exist renders as nothing, and a tab with no glyph
    /// is one nobody can identify. `NSImage` returns nil for a name the system
    /// does not have, so this is checkable rather than a hope.
    func testEveryTabHasASymbolTheSystemActuallyHas() {
        for tab in Tab.allCases {
            XCTAssertNotNil(
                NSImage(systemSymbolName: tab.symbol, accessibilityDescription: nil),
                "\(tab.name) asks for \(tab.symbol), which this system does not have")
        }
    }

    /// And no two tabs wear the same one, which would be four tabs and three
    /// distinguishable ones.
    func testNoTwoTabsShareASymbol() {
        XCTAssertEqual(
            Set(Tab.allCases.map(\.symbol)).count, Tab.allCases.count,
            "two tabs draw the same glyph")
    }

    // ---- the two lists, against the engine --------------------------------

    /// The four tabs, in the engine's order.
    func testTheTabsMatchTheEngine() throws {
        let tabs = try XCTUnwrap(fixture()["tabs"] as? [[String: Any]])
        XCTAssertEqual(tabs.count, Tab.allCases.count)
        for (i, entry) in tabs.enumerated() {
            XCTAssertEqual(entry["name"] as? String, Tab.allCases[i].name, "tab \(i)")
        }
    }

    /// And the five sections of the Colour tab — their titles, whether each
    /// starts open, and the effects each draws.
    func testTheSectionsMatchTheEngine() throws {
        let sections = try XCTUnwrap(fixture()["sections"] as? [[String: Any]])
        XCTAssertEqual(sections.count, Section.allCases.count)
        for (i, entry) in sections.enumerated() {
            let section = Section.allCases[i]
            XCTAssertEqual(entry["title"] as? String, section.title, "section \(i)")
            XCTAssertEqual(entry["starts_open"] as? Bool, section.startsOpen, section.title)
            XCTAssertEqual(entry["effects"] as? [String], section.effects, section.title)
        }
    }

    /// The warper and the mixer are shut to begin with, and nothing else is.
    /// Named rather than counted, so a third section quietly shut fails here.
    func testOnlyTheWarperAndTheMixerStartShut() {
        XCTAssertEqual(
            Section.allCases.filter { !$0.startsOpen }, [.colourWarper, .colourMixer])
    }

    // ---- the grouping -----------------------------------------------------

    /// Every pinned effect the registry declares is drawn by some section. One
    /// that is not would simply not appear anywhere, with nothing to say so —
    /// the worst kind of missing control.
    func testEveryPinnedEffectInTheRegistryIsReachable() throws {
        let registry = try JSONDecoder().decode(Registry.self, from: fixture("registry"))
        XCTAssertFalse(registry.pinned.isEmpty, "the fixture pins nothing")
        let owned = Section.allCases.flatMap(\.effects)
        XCTAssertEqual(
            Set(owned).count, owned.count,
            "a section claims an effect another section also claims")
        for key in registry.pinned {
            XCTAssertTrue(owned.contains(key), "\(key) is pinned but no section draws it")
        }
        // And nothing claimed that the registry does not pin: a fixed panel for
        // a row no document starts with is a panel with nothing behind it.
        for key in owned {
            XCTAssertTrue(
                registry.pinned.contains(key),
                "a section claims \(key), which is not pinned")
        }
    }

    /// ``Section/of(_:)`` answers about an effect *key*, not about a row. An
    /// effect nobody pinned has no fixed panel, and saying so is what lets the
    /// Effects tab draw it as a stack row rather than hunt for a section that
    /// does not exist.
    ///
    /// A row is a different question: the fixture's added row is a *second*
    /// exposure, whose key is pinned and whose row is not, which is why the
    /// inspector asks `row.pinned` first and this method second.
    func testAnEffectNobodyPinnedHasNoSection() {
        XCTAssertEqual(Section.of("colour_warper"), .colourWarper)
        XCTAssertEqual(Section.of("log_wheels"), .colourWheels)
        XCTAssertNil(Section.of("dehaze"))
        XCTAssertNil(Section.of("not_an_effect"))
    }

    /// And the added row really is one the pinned sections must not pick up:
    /// two rows carry the same key, and only the pinned one is a fixed panel.
    func testASectionTakesThePinnedRowAndNotAnAddedCopyOfIt() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        let added = try XCTUnwrap(snap.rows.first { !$0.pinned })
        XCTAssertTrue(
            Section.basic.effects.contains(added.effect),
            "the fixture no longer adds a second copy of a pinned effect, so this "
                + "checks nothing")
        let panel = try XCTUnwrap(
            snap.rows.first { $0.pinned && $0.effect == added.effect })
        XCTAssertNotEqual(panel.id, added.id)
    }

    /// Nothing in a document goes undrawn, and nothing is drawn twice.
    ///
    /// With one tab on screen, a row nothing claims is a control that exists,
    /// responds to nothing and appears nowhere — and there is no scroll far
    /// enough to find it. So the five sections and the Effects tab are checked
    /// as a *partition* of the document rather than one at a time.
    func testEveryRowIsDrawnExactlyOnce() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        XCTAssertFalse(snap.rows.isEmpty, "the fixture document has no rows")
        let drawn =
            Section.allCases.flatMap { $0.draws(snap.rows) }.map(\.id)
            + Tab.added(snap.rows).map(\.id)
        XCTAssertEqual(drawn.sorted(), snap.rows.map(\.id).sorted())
    }

    /// And each row is drawn at its own place in the stack, which is what the
    /// reorder arrows move by.
    func testARowKeepsItsPlaceInTheWholeStack() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        for drawn in Section.allCases.flatMap({ $0.draws(snap.rows) }) + Tab.added(snap.rows) {
            XCTAssertEqual(snap.rows[drawn.index].id, drawn.row.id)
        }
    }

    /// Image is the one tab the viewer changes for.
    ///
    /// `showsWholeFrame` is read twice in `ContentView` — once to put the
    /// overlay over the picture and once to tell the engine to frame the
    /// enclosing rectangle — so that the two cannot disagree. Left on after
    /// switching away, every other tab would be graded against a picture with
    /// the cut-away parts still in it.
    func testOnlyImageFramesTheWholeSource() {
        XCTAssertEqual(Tab.allCases.filter(\.showsWholeFrame), [.image])
    }

    // ---- the row ----------------------------------------------------------

    func testTheInspectorOpensOnColour() {
        XCTAssertEqual(Tab.allCases.first, .colour)
    }

    /// Each tab names what it will show, so a tab is not a guess.
    func testEveryTabHasAnAccessibilityLabel() {
        for tab in Tab.allCases {
            XCTAssertFalse(tab.name.isEmpty)
        }
    }

    /// What is remembered between launches is the raw value, so it has to read
    /// back as the tab it was written for. A renamed case would otherwise
    /// silently open on Colour every time.
    func testATabReadsBackAsWhatWasStored() {
        for tab in Tab.allCases {
            XCTAssertEqual(Tab(rawValue: tab.rawValue), tab)
        }
        XCTAssertNil(Tab(rawValue: "Image "), "the stored name is exact")
    }

    /// And so does a section's, which is what its folded-or-not state is keyed
    /// by. A renamed case unfolds everything the reader had folded.
    func testASectionReadsBackAsWhatWasStored() {
        for section in Section.allCases {
            XCTAssertEqual(Section(rawValue: section.rawValue), section)
        }
    }

    // ---- and against a live engine ----------------------------------------

    /// Adding an effect still works, and the row it adds appears where the
    /// reader is looking.
    ///
    /// The browser belongs to the Effects tab, which means the row it adds and
    /// the shelf that added it are the same screen. Asked of a real session
    /// rather than of a fixture, because "the engine accepted it" and
    /// "something draws it" are the two halves of the claim and only the first
    /// of them is about JSON.
    @MainActor
    func testAddingAnEffectPutsARowUnderTheEffectsTab() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()

        let before = Tab.added(store.snapshot.rows)
        XCTAssertTrue(before.isEmpty, "a fresh document has nothing added yet")

        let id = try XCTUnwrap(store.addEffect("sharpen"), store.problem ?? "refused")
        let after = Tab.added(store.snapshot.rows)
        XCTAssertEqual(after.map(\.id), [id], "the added row is not under Effects")
        XCTAssertEqual(after.first?.row.effect, "sharpen")

        // Still nothing lost and nothing doubled, now that the document has an
        // added row in it as well as the eleven pinned ones.
        let drawn =
            Section.allCases.flatMap { $0.draws(store.snapshot.rows) }.map(\.id)
            + Tab.added(store.snapshot.rows).map(\.id)
        XCTAssertEqual(drawn.sorted(), store.snapshot.rows.map(\.id).sorted())
    }

    /// Every pinned panel of a real document lands in some section, and each
    /// section's own order is what it draws in.
    @MainActor
    func testARealDocumentsPinnedPanelsAreAllReachable() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        XCTAssertFalse(store.snapshot.rows.isEmpty, "the chart did not open")

        for section in Section.allCases {
            XCTAssertEqual(
                section.draws(store.snapshot.rows).map(\.row.effect), section.effects,
                "\(section.title) does not draw what it claims")
        }
    }
}
