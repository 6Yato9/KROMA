import CoreGraphics
import SwiftUI
import XCTest

// Same module as the code under test; see EngineTests.swift.

/// The effect shelf: what it lists, in what order, and what a render of it
/// actually shows.
///
/// Two halves, and they fail for different reasons.
///
/// The first is `EffectBrowser.sections`, which is the whole of the rule about
/// order — starred first and only there, pinned rows nowhere, and no heading
/// over nothing. It is `apps/windows/src/inspector.rs`'s `browser`, in Swift,
/// against the real registry rather than a fixture: an effect added in Rust
/// appears in both shells with no change on either side, and a test written
/// against a hand-made list of effects would be a test of the hand-made list.
///
/// The second renders the tiles headlessly and reads the bitmap back, the way
/// `FilmstripTests`, `RowMetricsTests` and `CurveBackdropTests` do. What that
/// can check is where the ink lands and what colour it is: that a starred
/// effect's star is filled and an unstarred one's is not, and that the star
/// keeps to its own corner — which is the property that lets one tile carry two
/// gestures at all.
///
/// **What the render cannot reach.** `ImageRenderer` draws nothing inside a
/// `ScrollView`, so `EffectShelfList` is a view of its own and is the largest
/// piece of the shelf a headless test can be pointed at — the same arrangement
/// `FilmstripCells` has, and for the same reason. What stays out of reach is
/// the popover, the hover states and the two tap targets actually being
/// separate to a pointer; those stay unverified until somebody looks at it.
@MainActor
final class EffectBrowserTests: XCTestCase {

    // ---- what the shelf lists ---------------------------------------------

    /// Starred first, and only there. An effect in the list twice would be two
    /// tiles that do the same thing, one of them wrong the moment the star on
    /// the other is clicked.
    func testStarredEffectsSortToTheTopAndLeaveTheirOwnGroup() throws {
        let registry = try Engine.registry()
        let grain = try XCTUnwrap(registry.effect("grain"), "the registry has no grain")

        let plain = EffectBrowser.sections(in: registry, starred: [])
        XCTAssertFalse(
            plain.contains { $0.heading == "Favourites" },
            "a heading for favourites nobody has")
        XCTAssertTrue(
            plain.contains { $0.effects.contains { $0.key == "grain" } },
            "grain is not on offer at all")

        let starred = EffectBrowser.sections(in: registry, starred: ["grain"])
        let first = try XCTUnwrap(starred.first, "the shelf lists nothing")
        XCTAssertEqual(first.heading, "Favourites")
        XCTAssertEqual(first.effects.map(\.key), ["grain"])

        // And gone from the group it came from, rather than in both.
        for section in starred.dropFirst() {
            XCTAssertFalse(
                section.effects.contains { $0.key == "grain" },
                "grain is starred and still under \(section.heading)")
        }
        XCTAssertNotEqual(
            grain.group, "Favourites", "the registry already has a group by that name")
    }

    /// Every effect that can be added is on the shelf exactly once, starred or
    /// not. This is the half that catches a filter that drops something.
    func testEveryEffectThatCanBeAddedIsListedExactlyOnce() throws {
        let registry = try Engine.registry()
        let addable = Set(
            registry.effects.filter { !registry.pinned.contains($0.key) }.map(\.key))
        XCTAssertGreaterThan(addable.count, 5, "this is not the real registry")

        for starred in [[], ["grain"], ["grain", "halation"], Array(addable)] {
            let listed = EffectBrowser.sections(in: registry, starred: starred)
                .flatMap { $0.effects.map(\.key) }
            XCTAssertEqual(
                Set(listed), addable,
                "with \(starred.count) starred, the shelf lists a different set")
            XCTAssertEqual(
                listed.count, addable.count,
                "with \(starred.count) starred, something is listed twice")
        }
    }

    /// The pinned rows are the colour page's fixed panels. They are in every
    /// document already, so adding one would do nothing useful twice — and a
    /// star on one, however it got into the settings file, must not put it back
    /// on offer.
    func testAPinnedRowIsNotOnOfferEvenWhenItIsStarred() throws {
        let registry = try Engine.registry()
        let pinned = try XCTUnwrap(registry.pinned.first, "nothing is pinned")

        for starred in [[], [pinned]] {
            let listed = EffectBrowser.sections(in: registry, starred: starred)
                .flatMap { $0.effects.map(\.key) }
            XCTAssertFalse(
                listed.contains(pinned),
                "\(pinned) is a pinned row and is on the shelf anyway")
        }
    }

    /// A heading over nothing reads as a list that failed to load. Every Basic
    /// effect is pinned, so that group is empty before anybody starts starring
    /// — and a group whose entries have all been starred away is empty after.
    func testAHeadingWithNothingUnderItIsNotDrawn() throws {
        let registry = try Engine.registry()
        for section in EffectBrowser.sections(in: registry, starred: []) {
            XCTAssertFalse(section.effects.isEmpty, "\(section.heading) has nothing under it")
        }

        // A whole group starred away. Its heading has to go with it.
        let group = try XCTUnwrap(
            registry.groups.first { group in
                registry.effects.contains { $0.group == group && !registry.pinned.contains($0.key) }
            },
            "no group has anything addable in it")
        let all = registry.effects
            .filter { $0.group == group && !registry.pinned.contains($0.key) }
            .map(\.key)

        let sections = EffectBrowser.sections(in: registry, starred: all)
        XCTAssertFalse(
            sections.contains { $0.heading == group },
            "\(group) is drawn with all \(all.count) of its effects starred away")
        for section in sections {
            XCTAssertFalse(section.effects.isEmpty, "\(section.heading) has nothing under it")
        }
    }

    // ---- what a render shows ----------------------------------------------

    /// A starred effect's star is filled and an unstarred one's is an outline.
    ///
    /// Counted as gold ink rather than read at a point: the fill is a five-
    /// pointed star and a point inside it moves whenever the tile does. What
    /// cannot move is that a filled star is a great deal more gold than an
    /// outline of one, and that an outline is not gold at all until the pointer
    /// is on it.
    func testAStarredEffectIsFilledAndAnUnstarredOneIsNot() throws {
        let registry = try Engine.registry()
        let pair = try Self.two(from: registry)

        let none = try Self.render(Self.list(pair, starred: []))
        let one = try Self.render(Self.list(pair, starred: [pair[0].key]))
        let both = try Self.render(Self.list(pair, starred: pair.map(\.key)))

        XCTAssertEqual(
            Self.goldPixels(none), 0,
            "nothing is starred and the shelf drew a filled star anyway")
        let filled = Self.goldPixels(one)
        XCTAssertGreaterThan(filled, 10, "a starred effect drew \(filled) pixels of star")
        XCTAssertGreaterThan(
            Self.goldPixels(both), filled * 3 / 2,
            "two starred effects drew no more star than one")
    }

    /// The star keeps to its own corner.
    ///
    /// This is what makes one tile able to carry two gestures: `EffectTile`
    /// checks the star's target before the tile's own tap, exactly as
    /// `inspector.rs` does, and a star drawn anywhere else would be a tile
    /// where pressing the name sometimes stars and sometimes adds.
    func testTheStarKeepsToItsOwnCornerAndNotToTheName() throws {
        let registry = try Engine.registry()
        let pair = try Self.two(from: registry)
        let image = try Self.render(Self.list(pair, starred: pair.map(\.key)))

        let columns = Self.goldColumns(image)
        // Unwrapped rather than forced: a change that stops the star being
        // drawn at all is exactly what this is here to catch, and a test that
        // traps on it takes the whole run down instead of reporting.
        let leftmost = try XCTUnwrap(columns.min(), "no star was drawn at all")
        let rightmost = try XCTUnwrap(columns.max(), "no star was drawn at all")

        // The corner the star owns: `starInset` in from the tile's right edge,
        // and no wider than the target it is drawn inside.
        let right = EffectShelfList.width - EffectShelfList.inset
        let centre = right - EffectShelfList.starInset
        XCTAssertGreaterThanOrEqual(
            CGFloat(leftmost), centre - EffectShelfList.starTarget / 2,
            "a star reaches to column \(leftmost), which is over the name")
        XCTAssertLessThanOrEqual(
            CGFloat(rightmost), centre + EffectShelfList.starTarget / 2,
            "a star reaches to column \(rightmost), which is off the tile")
    }

    /// Five points about a centre, alternating an outer radius and an inner one
    /// at 0.44 of it — `inspector.rs`'s star, point for point. A four-pointed
    /// one, or one with the radii the wrong way round, is a different mark in
    /// an application that has one.
    func testTheStarHasFivePointsAboutItsCentre() {
        let box = CGRect(x: 0, y: 0, width: 12, height: 12)
        let centre = CGPoint(x: box.midX, y: box.midY)
        var corners: [CGPoint] = []
        Star().path(in: box).forEach { element in
            switch element {
            case .move(let to): corners.append(to)
            case .line(let to): corners.append(to)
            default: break
            }
        }
        XCTAssertEqual(corners.count, 10, "a star is ten corners")

        for (i, corner) in corners.enumerated() {
            let radius = hypot(corner.x - centre.x, corner.y - centre.y)
            let want = i.isMultiple(of: 2) ? 6.0 : 6.0 * 0.44
            XCTAssertEqual(
                radius, want, accuracy: 0.01,
                "corner \(i) is \(radius) from the middle")
        }
        // The first corner is straight up, which is what makes it a star
        // standing on two legs rather than lying on its side.
        XCTAssertEqual(corners[0].x, centre.x, accuracy: 0.01)
        XCTAssertLessThan(corners[0].y, centre.y)
    }

    // ---- the fixtures ------------------------------------------------------

    /// Two real effects, so the tiles are the ones the application draws.
    private static func two(from registry: Registry) throws -> [Effect] {
        let addable = registry.effects.filter { !registry.pinned.contains($0.key) }
        guard addable.count >= 2 else {
            throw Failure(what: "the registry has fewer than two addable effects")
        }
        return Array(addable.prefix(2))
    }

    private static func list(_ effects: [Effect], starred: [String]) -> some View {
        let kept = Set(starred)
        return EffectShelfList(
            sections: [EffectSection(heading: "Effects", effects: effects)],
            isStarred: { kept.contains($0) },
            add: { _ in },
            star: { _ in }
        )
        .padding(EffectShelfList.inset)
    }

    private struct Failure: Error, CustomStringConvertible {
        let what: String
        var description: String { what }
    }

    // ---- reading the render ------------------------------------------------

    private static func render<V: View>(_ view: V) throws -> CGImage {
        let renderer = ImageRenderer(
            content: view
                // Pinned to the top left: anything smaller than the frame it is
                // given is centred in it otherwise, and every column this file
                // reads is counted from the shelf's own corner.
                .frame(
                    width: EffectShelfList.width, height: 4 * EffectShelfList.stride + 40,
                    alignment: .topLeading
                )
                .background(Color.black)
                .environment(\.colorScheme, .dark))
        renderer.scale = 1
        return try XCTUnwrap(renderer.cgImage, "the renderer produced no image")
    }

    private static func bytes(_ image: CGImage) -> [UInt8] {
        let (w, h) = (image.width, image.height)
        var out = [UInt8](repeating: 0, count: w * h * 4)
        if let context = CGContext(
            data: &out, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        {
            context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        }
        return out
    }

    /// Whether a pixel is the star's gold — `WARN`, which is the palette's one
    /// warm colour and is nothing else on this shelf. `ACCENT` is the hovered
    /// tile's outline and is a red: it fails the green test below, which is
    /// what keeps the two apart.
    private static func isGold(_ r: Int, _ g: Int, _ b: Int) -> Bool {
        r > 150 && r > b + 60 && g > b + 30
    }

    private static func goldPixels(_ image: CGImage) -> Int {
        let data = bytes(image)
        var count = 0
        for y in 0..<image.height {
            for x in 0..<image.width {
                let i = (y * image.width + x) * 4
                if isGold(Int(data[i]), Int(data[i + 1]), Int(data[i + 2])) { count += 1 }
            }
        }
        return count
    }

    private static func goldColumns(_ image: CGImage) -> [Int] {
        let data = bytes(image)
        var columns: Set<Int> = []
        for y in 0..<image.height {
            for x in 0..<image.width {
                let i = (y * image.width + x) * 4
                if isGold(Int(data[i]), Int(data[i + 1]), Int(data[i + 2])) { columns.insert(x) }
            }
        }
        return columns.sorted()
    }
}
