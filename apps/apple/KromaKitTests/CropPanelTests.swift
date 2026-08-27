import AppKit
import CoreGraphics
import XCTest

// Same module as the code under test; see EngineTests.swift.

/// The crop tool's panel.
///
/// Three things here cannot be seen by the compiler and are not visible on
/// screen either until somebody notices something missing.
///
/// The glyphs are *strings* handed to the system, so a name the system does not
/// have is a button that draws nothing — the same hazard `TabRowTests` asks
/// about for the strip.
///
/// The aspect names are a table read in both directions, and the lock they name
/// loses its spelling crossing the ABI: 16:9 goes out as one float and comes
/// back as `.ratio(w: 1.777…, h: 1)`. A menu that matched on the two numbers
/// would quietly stop saying "16:9" about a lock that is still 16:9.
///
/// And the panel proposes a geometry and then draws what the engine stored,
/// which are different values whenever the engine corrects — a turn below zero,
/// a lock that re-shapes the crop, a straighten that pulls it inside the
/// photograph. Those are asked of a real session, because "what the engine
/// corrects" is exactly what a fixture on this side would be guessing at.
final class CropPanelTests: XCTestCase {

    // ---- the glyphs -------------------------------------------------------

    /// A symbol the system does not have renders as nothing, and a blank button
    /// on a row of four is one nobody can identify.
    ///
    /// This is not idle: `rotate.left` and `rotate.right` are the two drawings
    /// that say "quarter-turn" most plainly, and they are SF Symbols 6 — macOS
    /// 15, above the 14.0 deployment target. They resolve on a machine new
    /// enough to build this and would be blank on one that is not.
    func testEveryGlyphOnThePanelIsASymbolTheSystemHas() {
        for symbol in CropPanel.symbols {
            XCTAssertNotNil(
                NSImage(systemSymbolName: symbol, accessibilityDescription: nil),
                "the panel asks for \(symbol), which this system does not have")
        }
    }

    /// And no two of the four wear the same one, which is four buttons and
    /// three distinguishable ones.
    func testNoTwoControlsOnTheRowShareAGlyph() {
        XCTAssertEqual(Set(CropPanel.symbols).count, CropPanel.symbols.count)
    }

    // ---- the aspect names -------------------------------------------------

    /// Every preset is named by the panel and read back by it, so the menu's
    /// tick lands on the entry that was chosen.
    func testTheAspectPresetsRoundTripThroughTheirNames() {
        for preset in CropPanel.aspects {
            XCTAssertEqual(
                CropPanel.name(of: preset.lock), preset.name,
                "\(preset.name) is not what the panel calls its own lock")
            XCTAssertEqual(
                CropPanel.lock(named: preset.name), preset.lock,
                "\(preset.name) does not read back as the lock it names")
        }
    }

    /// A ratio that has been across the ABI is the same lock spelled with a
    /// denominator of one, and the menu has to go on calling it what it is.
    ///
    /// This is the state the panel is actually in mid-drag: `store.geometry` is
    /// then the engine's corrected answer, whose aspect came back through one
    /// float.
    func testARatioThatLostItsSpellingIsStillNamed() {
        XCTAssertEqual(CropPanel.name(of: .ratio(w: 16.0 / 9.0, h: 1)), "16:9")
        XCTAssertEqual(CropPanel.name(of: .ratio(w: 1.5, h: 1)), "3:2")
        XCTAssertEqual(CropPanel.name(of: .ratio(w: 1, h: 1)), "1:1")
    }

    /// A ratio the panel does not offer — from a later version, or from the
    /// Windows shell — is printed rather than dropped, and the menu lists it so
    /// the tick has somewhere to land. Showing "Free" for a crop that is locked
    /// would be a lie about what the next drag does.
    func testARatioThePanelDoesNotOfferIsStillShown() {
        let cinema = AspectLock.ratio(w: 2.35, h: 1)
        XCTAssertEqual(CropPanel.name(of: cinema), "2.35:1")
        XCTAssertTrue(CropPanel.options(showing: cinema).contains("2.35:1"))
        // And it is not mistaken for a preset on the way back, which would turn
        // merely *looking* at the menu into an edit.
        XCTAssertNil(CropPanel.lock(named: "2.35:1"))
        // The presets need no extra entry.
        XCTAssertEqual(
            CropPanel.options(showing: .original), CropPanel.aspects.map(\.name))
    }

    // ---- what a control asks for ------------------------------------------

    /// One field changes and the other six are carried across untouched. The
    /// crop especially: turning the picture must not also move the rectangle,
    /// which is the engine's business and only when it has to.
    func testAProposalCarriesEverythingItWasNotAskedToChange() {
        let held = GeometryValue(
            centre: CGPoint(x: 0.1, y: -0.05), size: CGSize(width: 0.5, height: 0.4),
            angle: 7, turns: 2, flipH: true, flipV: false, aspect: .ratio(w: 3, h: 2))

        let turned = CropPanel.proposed(from: held, turns: held.turns + 1)
        XCTAssertEqual(turned.turns, 3)
        XCTAssertEqual(turned.centre, held.centre)
        XCTAssertEqual(turned.size, held.size)
        XCTAssertEqual(turned.angle, held.angle)
        XCTAssertEqual(turned.flipH, held.flipH)
        XCTAssertEqual(turned.aspect, held.aspect)

        let straightened = CropPanel.proposed(from: held, angle: -3)
        XCTAssertEqual(straightened.angle, -3)
        XCTAssertEqual(straightened.turns, held.turns)
        XCTAssertEqual(straightened.aspect, held.aspect)

        let unlocked = CropPanel.proposed(from: held, aspect: .free)
        XCTAssertEqual(unlocked.aspect, .free)
        XCTAssertEqual(unlocked.size, held.size)
    }

    // ---- and against a live engine ----------------------------------------

    @MainActor
    private func opened() throws -> SessionStore {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart(width: 1024, height: 768)
        XCTAssertNil(store.problem)
        return store
    }

    /// The anticlockwise button asks for one turn *less* than the document
    /// holds, which on an unturned photograph is minus one — a count no
    /// document may hold. The engine takes it modulo four and the panel draws
    /// the answer, so the row never shows a negative turn.
    ///
    /// This is the plainest case of "draw what came back": the proposal and the
    /// stored value are different numbers, and one of them is not a geometry.
    @MainActor
    func testAnAnticlockwiseTurnIsDrawnAsTheEngineStoredIt() throws {
        let store = try opened()
        let asked = CropPanel.proposed(from: store.geometry, turns: store.geometry.turns - 1)
        XCTAssertEqual(asked.turns, -1, "the panel asks for one less than it has")

        store.setGeometry(asked)
        XCTAssertNil(store.problem)
        XCTAssertEqual(store.geometry.turns, 3, "the panel would have drawn -1")

        // And the clockwise button wraps at the other end: a fourth quarter-turn
        // is none at all, which is the same `% 4` seen from above.
        store.setGeometry(
            CropPanel.proposed(from: store.geometry, turns: store.geometry.turns + 1))
        XCTAssertEqual(store.geometry.turns, 0, "the panel would have drawn 4")
    }

    /// Choosing a lock re-shapes the crop there and then — the engine's
    /// `apply_aspect` — so the size that comes back is not the size that went
    /// in. A panel drawing its own proposal would show a full-frame crop beside
    /// an overlay drawing a square one.
    @MainActor
    func testALockedAspectReshapesTheCropAndThePanelDrawsTheResult() throws {
        let store = try opened()
        XCTAssertEqual(store.geometry.size, CGSize(width: 1, height: 1))

        let asked = CropPanel.proposed(from: store.geometry, aspect: .ratio(w: 1, h: 1))
        XCTAssertEqual(asked.size, CGSize(width: 1, height: 1), "the panel asks for no resize")

        store.setGeometry(asked)
        XCTAssertNil(store.problem)
        let stored = store.geometry
        XCTAssertNotEqual(
            stored.size, asked.size,
            "the whole of a 4:3 photograph is not a square, so the engine had to shrink it")
        // Square in pixels, which on a 1024x768 source is not square in the
        // fractions the geometry is stored in.
        XCTAssertEqual(
            Double(stored.size.width) * 1024, Double(stored.size.height) * 768,
            accuracy: 1,
            "the crop the engine kept is not square")
        XCTAssertEqual(CropPanel.name(of: stored.aspect), "1:1", "the menu lost the lock")
    }

    /// A straighten pulls the crop inside the photograph — there is no picture
    /// in the corners a rotation opens up — so the size comes back smaller than
    /// it went in while the angle comes back exactly as asked.
    @MainActor
    func testAStraightenKeepsItsAngleAndCostsTheCropSomeSize() throws {
        let store = try opened()
        let asked = CropPanel.proposed(from: store.geometry, angle: 15)

        store.setGeometry(asked)
        XCTAssertNil(store.problem)
        XCTAssertEqual(store.geometry.angle, 15, accuracy: 1e-4, "the slider would jump")
        XCTAssertLessThan(
            Double(store.geometry.size.width), 1,
            "a straightened crop that still fills the frame has blank corners in it")
    }

    /// Reset is the way back: the crop, the angle, the turns, the flips and the
    /// lock all at once.
    @MainActor
    func testResetPutsTheWholePhotographBack() throws {
        let store = try opened()
        store.setGeometry(
            GeometryValue(
                centre: CGPoint(x: 0.05, y: 0), size: CGSize(width: 0.4, height: 0.4),
                angle: 8, turns: 1, flipH: true, flipV: false, aspect: .ratio(w: 1, h: 1)))
        XCTAssertFalse(store.geometry.isIdentity)

        store.resetGeometry()
        XCTAssertNil(store.problem)
        XCTAssertTrue(store.geometry.isIdentity, "\(store.geometry) is not the whole photograph")
        XCTAssertEqual(CropPanel.name(of: store.geometry.aspect), "Free")
    }

    // ---- the tool turning the viewer on and off ---------------------------

    /// Choosing the Image tab frames the viewer on the enclosing rectangle, and
    /// choosing any other tab puts it back.
    ///
    /// The rule is `Tab.showsWholeFrame`, which is what `ContentView` hands to
    /// `setCropping` on every change of tab — so this drives the same value
    /// through the same call the interface makes. What stays unverified on this
    /// side is SwiftUI's `onChange` actually firing; what is checked is that
    /// the tabs answer the question correctly and that the store does the right
    /// thing with both answers.
    ///
    /// Left on after switching away, every other tab would be graded against a
    /// picture with the cut-away parts still in it.
    @MainActor
    func testSwitchingToImageFramesTheWholeSourceAndSwitchingAwayPutsItBack() throws {
        let store = try opened()
        store.setGeometry(
            GeometryValue(
                centre: .zero, size: CGSize(width: 0.5, height: 0.5), angle: 0, turns: 0,
                flipH: false, flipV: false, aspect: .free))

        store.setCropping(Tab.image.showsWholeFrame)
        XCTAssertTrue(store.cropping, "the Image tab did not open the enclosing frame")
        XCTAssertEqual(Double(store.cropRect.width), 0.5, accuracy: 2e-3)
        XCTAssertEqual(Double(store.cropRect.height), 0.5, accuracy: 2e-3)

        for tab in Tab.allCases where tab != .image {
            store.setCropping(tab.showsWholeFrame)
            XCTAssertFalse(
                store.cropping,
                "\(tab.name) left the viewer showing what the crop cuts away")
            // The viewer is showing the crop itself again, so the crop fills it.
            XCTAssertEqual(Double(store.cropRect.width), 1, accuracy: 1e-3, tab.name)
            XCTAssertEqual(Double(store.cropRect.height), 1, accuracy: 1e-3, tab.name)
            store.setCropping(Tab.image.showsWholeFrame)
        }

        // And none of that was an edit: the framing is a property of the
        // viewer, not of the document.
        XCTAssertEqual(Double(store.geometry.size.width), 0.5, accuracy: 1e-6)
    }
}
