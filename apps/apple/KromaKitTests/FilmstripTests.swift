import CoreGraphics
import SwiftUI
import XCTest

// Same module as the code under test; see EngineTests.swift.

/// The filmstrip: which cells it asks the engine for, and what a render of it
/// actually shows.
///
/// Two halves, and they fail for different reasons.
///
/// The first is arithmetic — `Filmstrip.visible`, `Filmstrip.wanted` and the
/// `Filmstrip.asking` that composes them, which between them decide how much
/// work the strip does per frame. It is the same set of cases
/// `apps/windows/src/filmstrip.rs` pins for its own `visible`, because it is
/// the same function: a strip that asked for every thumbnail in a folder of a
/// thousand would hand back the memory the whole design exists to save.
///
/// The second renders the strip headlessly and reads the bitmap back, the way
/// `RowMetricsTests`, `CurveBackdropTests`, `WarperCloudTests` and
/// `CropOverlayTests` do. What that can check is where the ink lands and what
/// colour it is: that the photograph on screen is marked and the others are
/// not, that a cell whose thumbnail has not arrived still holds its place, that
/// the cells sit on the very stride the arithmetic divides by, and that a set
/// of one gets no strip at all.
///
/// **What the render cannot reach.** `ImageRenderer` draws nothing inside a
/// `ScrollView` — a scroll view has no viewport off screen, so its content is
/// never laid out. That is why `FilmstripCells` is a view of its own: it is the
/// largest piece of the real strip a headless test can be pointed at, and it is
/// the piece the cells' geometry lives in. What stays out of reach is the
/// scrolling itself, and with it the `GeometryReader` that turns a scroll
/// offset into a request — so the tie between the two halves of this file is
/// `testTheCellsSitOnTheStrideTheArithmeticDividesBy`, which checks that the
/// cells really are laid out on the number `visible` divides by, rather than a
/// render that watches a request go out.
///
/// And whether any of it is *pleasant* — or whether the column sits where it
/// should beside a viewer that is drawing into a Metal layer — stays unverified
/// until somebody looks at it.
final class FilmstripTests: XCTestCase {

    // ---- which cells are on screen ----------------------------------------

    /// A window five cells tall, scrolled ten cells down, over a folder of a
    /// thousand.
    func testOnlyTheCellsOnScreenAreVisible() {
        let stride = Filmstrip.stride
        let range = Filmstrip.visible(
            from: 10 * stride, to: 15 * stride, stride: stride, count: 1000)
        XCTAssertEqual(range.lowerBound, 10)
        XCTAssertLessThanOrEqual(
            range.upperBound, 16, "\(range) is more than the window can hold")
    }

    /// The property the whole strip rests on: what it costs per frame is set by
    /// the size of the window, not by how many photographs are open.
    func testTheCostDoesNotGrowWithTheNumberOfPhotographs() {
        let stride = Filmstrip.stride
        let small = Filmstrip.visible(from: 0, to: 8 * stride, stride: stride, count: 20)
        let huge = Filmstrip.visible(from: 0, to: 8 * stride, stride: stride, count: 100_000)
        XCTAssertEqual(huge.count, small.count)
        XCTAssertLessThan(huge.count, 12, "a wide-open folder drew \(huge.count) cells")
    }

    func testTheRangeStopsAtTheLastPhotograph() {
        let stride = Filmstrip.stride
        XCTAssertEqual(
            Filmstrip.visible(from: 0, to: 50 * stride, stride: stride, count: 3), 0..<3)
    }

    func testAnEmptySetAsksForNothing() {
        XCTAssertEqual(
            Filmstrip.visible(from: 0, to: 800, stride: Filmstrip.stride, count: 0), 0..<0)
        XCTAssertEqual(Filmstrip.wanted(showing: 0..<0, count: 0), 0..<0)
        // A strip laid out before it has been given a size is not a division by
        // zero, and a geometry that has not resolved is not a range built out
        // of a NaN. Either would be a crash rather than a strip.
        XCTAssertEqual(Filmstrip.visible(from: 0, to: 100, stride: 0, count: 9), 0..<0)
        XCTAssertEqual(
            Filmstrip.visible(from: .nan, to: .nan, stride: Filmstrip.stride, count: 9), 0..<0)
    }

    /// Scrolled past the end — which an elastic overscroll does momentarily —
    /// must not produce a range that starts after it ends. In Swift that is not
    /// an empty range but a trap.
    func testScrollingPastTheEndIsNotAnInvertedRange() {
        let stride = Filmstrip.stride
        let range = Filmstrip.visible(
            from: 90 * stride, to: 95 * stride, stride: stride, count: 5)
        XCTAssertLessThanOrEqual(range.lowerBound, range.upperBound, "\(range)")
        XCTAssertTrue(range.isEmpty)
        XCTAssertEqual(Filmstrip.wanted(showing: range, count: 5), 0..<0)

        // The same guarantee for a window handed over back to front. Nothing
        // in the column produces one — `asking` passes `top` and `top + height`
        // and a height is never negative — but that is the shape of argument
        // that turns a missing clamp into a crash rather than a wrong picture,
        // and a strip that traps is worse than a strip that draws nothing.
        let backwards = Filmstrip.visible(
            from: 5 * stride, to: 0, stride: stride, count: 100)
        XCTAssertTrue(backwards.isEmpty, "a window given back to front made \(backwards)")
    }

    /// The look-ahead reaches past the view, and nowhere near through the set.
    ///
    /// Eight cells, which is `filmstrip.rs`'s own `LOOKAHEAD`: enough that a
    /// thumbnail is usually there by the time it scrolls into sight, few enough
    /// that opening a large folder does not queue hundreds of decodes for
    /// photographs nobody has looked at.
    func testTheLookAheadReachesPastTheViewButNotThroughTheSet() {
        XCTAssertGreaterThan(Filmstrip.lookAhead, 0, "there is no look-ahead at all")

        let stride = Filmstrip.stride
        let onScreen = Filmstrip.visible(
            from: 10 * stride, to: 15 * stride, stride: stride, count: 1000)
        let asked = Filmstrip.wanted(showing: onScreen, count: 1000)

        XCTAssertEqual(asked.lowerBound, onScreen.lowerBound, "the ask skipped a visible cell")
        XCTAssertGreaterThan(
            asked.upperBound, onScreen.upperBound, "nothing is asked for ahead of the view")
        XCTAssertEqual(asked.upperBound, onScreen.upperBound + Filmstrip.lookAhead)
        // And at the end of the set the look-ahead stops rather than running
        // off it.
        XCTAssertEqual(Filmstrip.wanted(showing: 995..<1000, count: 1000), 995..<1000)
    }

    /// What the column actually asks for, composed the way it composes it: the
    /// stride the cells sit on, both ends of the window, and the look-ahead
    /// applied once.
    ///
    /// The number that matters is the last one. A five-cell window over a
    /// folder of a thousand asks for fourteen thumbnails; a strip that asked
    /// for the set would be asking for a thousand decodes and, at 64 KB a
    /// thumbnail, thirteen megabytes of pictures nobody has looked at.
    func testAColumnAsksForItsOwnWindowAndNotForTheFolder() {
        let stride = Filmstrip.stride
        XCTAssertEqual(
            Filmstrip.asking(top: 10 * stride, height: 5 * stride, count: 1000), 10..<24)
        XCTAssertEqual(Filmstrip.asking(top: 0, height: 3 * stride, count: 1000), 0..<12)

        let asked = Filmstrip.asking(top: 0, height: 5 * stride, count: 1000)
        XCTAssertLessThan(
            asked.count, 20,
            "a five-cell window asked for \(asked.count) thumbnails of a set of 1000")

        // A window over a set of three is a window over three.
        XCTAssertEqual(Filmstrip.asking(top: 0, height: 50 * stride, count: 3), 0..<3)
        // And a session with no set asks for nothing at all.
        XCTAssertEqual(Filmstrip.asking(top: 0, height: 400, count: 0), 0..<0)
    }

    /// Two folders of the same size are not one folder.
    ///
    /// The ask is keyed on the set as well as on the range, and this is the
    /// half that is easy to leave out: opening a second folder of ten
    /// photographs behind the first leaves the window exactly where it was, so
    /// a strip that could only see the range would never ask for the second
    /// folder's thumbnails and would sit there showing ten empty frames.
    func testTwoDifferentSetsOfTheSameSizeAreToldApart() {
        let set = Self.entries(3)
        XCTAssertEqual(
            Filmstrip.identity(of: set), Filmstrip.identity(of: Self.entries(3)),
            "the same three photographs read as two different sets")

        let elsewhere = set.map {
            LibraryEntry(
                index: $0.index,
                path: URL(fileURLWithPath: "/tmp/another-folder/\($0.name)"),
                edited: false, failed: false, hasThumbnail: false)
        }
        XCTAssertNotEqual(
            Filmstrip.identity(of: set), Filmstrip.identity(of: elsewhere),
            "another folder of three photographs reads as the same set")

        // A photograph taken out of the set is a different set, even though the
        // one before and the one after it are unchanged.
        XCTAssertNotEqual(
            Filmstrip.identity(of: set),
            Filmstrip.identity(of: [set[0], set[2]]),
            "a set with a photograph removed reads as the set it came from")

        // And the marks are not part of it: a thumbnail arriving, or an edit
        // being parked, is not a new set to ask for.
        let arrived = set.map {
            LibraryEntry(
                index: $0.index, path: $0.path, edited: true, failed: false,
                hasThumbnail: true)
        }
        XCTAssertEqual(
            Filmstrip.identity(of: set), Filmstrip.identity(of: arrived),
            "a thumbnail arriving read as a new set, which is an ask per thumbnail")
    }

    /// A set worth a strip is a set with more than one photograph in it, which
    /// is what `main.rs` decides.
    func testOnlyASetWorthNavigatingGetsAStrip() {
        XCTAssertFalse(Filmstrip.isWorthShowing(count: 0))
        XCTAssertFalse(Filmstrip.isWorthShowing(count: 1))
        XCTAssertTrue(Filmstrip.isWorthShowing(count: 2))
    }

    // ---- what a render shows ----------------------------------------------

    /// The photograph on screen is marked, and the rest are not.
    ///
    /// Read in the cell's own margin beside the name rather than on the frame,
    /// because that is where the backing shows: `SELECT` is a muted blue and
    /// `PANEL` is grey, so the check is that one cell's band is blue and its
    /// neighbours' are not.
    @MainActor
    func testTheCurrentEntryIsMarkedAndTheOthersAreNot() throws {
        let image = try Self.render(
            Self.cells(Self.entries(4), current: 1), height: 4 * Filmstrip.stride)

        // Well inside the left margin of the cell, on the line the name is
        // written on — the name is centred and short, so nothing is drawn here.
        let x = Int(Filmstrip.margin + Filmstrip.pad) + 2
        let marked = Self.rgb(image, x: x, y: Self.captionRow(1))
        XCTAssertGreaterThan(
            marked.b, marked.r + 40,
            "the photograph on screen is not marked — its cell read \(marked)")

        for other in [0, 2, 3] {
            let plain = Self.rgb(image, x: x, y: Self.captionRow(other))
            XCTAssertLessThan(
                plain.b, plain.r + 10,
                "entry \(other) is marked as well as the one on screen — it read \(plain)")
        }
    }

    /// A cell whose thumbnail has not arrived holds its place.
    ///
    /// Everything below it would otherwise shuffle up the column and back down
    /// again, one cell at a time, as a folder of thumbnails came in off the
    /// worker thread.
    @MainActor
    func testAnEntryWithNoThumbnailStillOccupiesItsPlace() throws {
        let height = 3 * Filmstrip.stride
        let red = Self.picture(220, 0, 0)
        let green = Self.picture(0, 220, 0)

        let filled = try Self.render(
            Self.cells(
                Self.entries(3, thumbs: [0, 1, 2]),
                pictures: [0: red, 1: red, 2: green]),
            height: height)
        let waiting = try Self.render(
            Self.cells(Self.entries(3, thumbs: [2]), pictures: [2: green]),
            height: height)

        let withNeighbours = try XCTUnwrap(
            Self.firstGreenRow(filled), "the third photograph was not drawn at all")
        let alone = try XCTUnwrap(
            Self.firstGreenRow(waiting), "the third photograph was not drawn at all")
        XCTAssertLessThanOrEqual(
            abs(alone - withNeighbours), 1,
            "the third frame moved from row \(withNeighbours) to row \(alone) when the two "
                + "above it had no thumbnail yet — the cells collapsed")
    }

    /// The cells sit on the very stride the arithmetic divides by.
    ///
    /// This is what ties the two halves of the file together. `visible` turns a
    /// scroll offset into cell indices by dividing by `Filmstrip.stride`; if
    /// the column lays them out on some other pitch then every range the strip
    /// asks for is a range for the wrong photographs, and no amount of checking
    /// the arithmetic on its own would say so.
    @MainActor
    func testTheCellsSitOnTheStrideTheArithmeticDividesBy() throws {
        let red = Self.picture(220, 0, 0)
        let image = try Self.render(
            Self.cells(
                Self.entries(4, thumbs: [0, 1, 2, 3]),
                pictures: [0: red, 1: red, 2: red, 3: red]),
            height: 4 * Filmstrip.stride)

        let tops = Self.frameTops(image)
        XCTAssertEqual(tops.count, 4, "\(tops.count) frames were drawn, not four")
        for i in tops.indices.dropFirst() {
            XCTAssertEqual(
                CGFloat(tops[i] - tops[i - 1]), Filmstrip.stride, accuracy: 1,
                "cell \(i) starts \(tops[i] - tops[i - 1]) points below cell \(i - 1), and "
                    + "the arithmetic divides by \(Filmstrip.stride)")
        }
    }

    /// The mark on a photograph that has an edit parked on it, so a set half
    /// way through a pass is readable at a glance — and only on those.
    @MainActor
    func testTheEditedMarkIsOnlyOnThePhotographsThatHaveOne() throws {
        let image = try Self.render(
            Self.cells(Self.entries(3, edited: [1]), current: 0),
            height: 3 * Filmstrip.stride)

        for i in 0..<3 {
            // Strictly inside the frame's top-right corner, so the reading is
            // the mark or the empty well and never the cell's own backing.
            let corner = Self.brightest(
                image,
                columns: Self.frameRight - 16...Self.frameRight - 3,
                rows: Self.frameTop(i) + 2...Self.frameTop(i) + 16)
            if i == 1 {
                XCTAssertGreaterThan(
                    corner, 150,
                    "the edited photograph carries no mark — its corner read \(corner)")
            } else {
                XCTAssertLessThan(
                    corner, 90,
                    "entry \(i) has no edit parked on it and is marked anyway — \(corner)")
            }
        }
    }

    /// The name under each frame. A column of thumbnails from one shoot is a
    /// column of very similar pictures, and the name is often the only thing
    /// that tells two of them apart.
    @MainActor
    func testTheNameIsWrittenUnderEveryFrame() throws {
        let image = try Self.render(
            Self.cells(Self.entries(3), current: nil), height: 3 * Filmstrip.stride)
        for i in 0..<3 {
            let ink = Self.contrast(
                image,
                columns: Int(Filmstrip.margin)...Int(Filmstrip.width - Filmstrip.margin) - 1,
                rows: Self.captionRow(i) - 4...Self.captionRow(i) + 4)
            XCTAssertGreaterThan(ink, 50, "nothing is written under frame \(i)")
        }
    }

    /// One photograph does not need navigating, so it gets no strip — not an
    /// empty one, and not a column of one.
    ///
    /// Read on the whole column rather than on the cells, because what has to
    /// disappear is the column itself: its panel, its rule and the width it
    /// takes out of the window. The cells inside a `ScrollView` do not render
    /// headlessly, so what this weighs is that furniture.
    @MainActor
    func testASetOfOnePhotographGetsNoStripAtAll() throws {
        let alone = try Self.render(
            Self.column(Self.entries(1)), height: 3 * Filmstrip.stride, width: 200)
        XCTAssertEqual(Self.inkedPixels(alone), 0, "a set of one photograph drew a strip")

        // The same render of a set of two is not blank, so the check above is a
        // check on the rule and not on a renderer that draws nothing.
        let pair = try Self.render(
            Self.column(Self.entries(2)), height: 3 * Filmstrip.stride, width: 200)
        XCTAssertGreaterThan(Self.inkedPixels(pair), 1000, "a set of two drew no strip")
        // And what it drew is the column's own width and no more, so the strip
        // is a column down the side rather than something across the window.
        XCTAssertEqual(
            Self.lastInkedColumn(pair), Int(Filmstrip.width) - 1,
            "the strip is not \(Filmstrip.width) points wide")
    }

    // ---- the fixtures ------------------------------------------------------

    private static func entries(
        _ count: Int, edited: Set<Int> = [], failed: Set<Int> = [], thumbs: Set<Int> = []
    ) -> [LibraryEntry] {
        (0..<count).map { i in
            LibraryEntry(
                index: i,
                path: URL(fileURLWithPath: "/tmp/kroma-filmstrip/\(Self.name(i)).png"),
                edited: edited.contains(i),
                failed: failed.contains(i),
                hasThumbnail: thumbs.contains(i))
        }
    }

    private static func name(_ i: Int) -> String {
        String(UnicodeScalar(UInt8(97 + i % 26)))
    }

    /// The cells, which is as much of the strip as a headless render reaches.
    private static func cells(
        _ entries: [LibraryEntry],
        current: Int? = 0,
        pictures: [Int: CGImage] = [:]
    ) -> FilmstripCells {
        FilmstripCells(
            entries: entries, current: current,
            picture: { pictures[$0.index] }, show: { _ in })
    }

    /// The whole column, panel and rule and all.
    private static func column(
        _ entries: [LibraryEntry], current: Int? = 0
    ) -> FilmstripColumn {
        FilmstripColumn(
            entries: entries, current: current,
            picture: { _ in nil }, ask: { _ in }, show: { _ in })
    }

    /// A flat picture the size of the frame's inside, so that it fits exactly
    /// and a row of it is a row of one colour.
    private static func picture(_ r: UInt8, _ g: UInt8, _ b: UInt8) -> CGImage {
        let width = Int(Filmstrip.frame.width) - 4
        let height = Int(Filmstrip.frame.height) - 4
        var rgba: [UInt8] = []
        rgba.reserveCapacity(width * height * 4)
        for _ in 0..<(width * height) { rgba.append(contentsOf: [r, g, b, 255]) }
        return Thumbnail(width: width, height: height, rgba: rgba).image!
    }

    // ---- where things are, in the render -----------------------------------

    /// The top of cell `i`'s frame.
    private static func frameTop(_ i: Int) -> Int {
        Int(CGFloat(i) * Filmstrip.stride + Filmstrip.pad)
    }

    /// The right-hand edge of a frame, which is where the edited mark sits.
    private static var frameRight: Int {
        Int(Filmstrip.margin + Filmstrip.pad + Filmstrip.frame.width)
    }

    /// The middle of the line cell `i`'s name is written on.
    private static func captionRow(_ i: Int) -> Int {
        Int(
            CGFloat(i) * Filmstrip.stride + Filmstrip.pad + Filmstrip.frame.height
                + Filmstrip.inside + Filmstrip.caption / 2)
    }

    // ---- reading the render ------------------------------------------------

    @MainActor
    private static func render<V: View>(
        _ view: V, height: CGFloat, width: CGFloat = Filmstrip.width
    ) throws -> CGImage {
        let renderer = ImageRenderer(
            content: view
                // Pinned to the top left: anything smaller than the frame it
                // is given is centred in it otherwise, and every row and
                // column this file reads is counted from the strip's own
                // corner.
                .frame(width: width, height: height, alignment: .topLeading)
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

    private static func rgb(_ image: CGImage, x: Int, y: Int) -> (r: Int, g: Int, b: Int) {
        let data = bytes(image)
        guard x >= 0, y >= 0, x < image.width, y < image.height else { return (0, 0, 0) }
        let i = (y * image.width + x) * 4
        return (Int(data[i]), Int(data[i + 1]), Int(data[i + 2]))
    }

    /// The first row holding a pixel that is unmistakably the green fixture
    /// picture rather than any grey in the palette.
    private static func firstGreenRow(_ image: CGImage) -> Int? {
        let data = bytes(image)
        for y in 0..<image.height {
            for x in 0..<image.width {
                let i = (y * image.width + x) * 4
                let (r, g, b) = (Int(data[i]), Int(data[i + 1]), Int(data[i + 2]))
                if g > 120, g > r + 60, g > b + 60 { return y }
            }
        }
        return nil
    }

    /// The top row of each run of rows holding the red fixture picture — that
    /// is, where each frame begins.
    private static func frameTops(_ image: CGImage) -> [Int] {
        let data = bytes(image)
        var tops: [Int] = []
        var inFrame = false
        for y in 0..<image.height {
            var found = false
            for x in 0..<image.width {
                let i = (y * image.width + x) * 4
                let (r, g, b) = (Int(data[i]), Int(data[i + 1]), Int(data[i + 2]))
                if r > 120, r > g + 60, r > b + 60 {
                    found = true
                    break
                }
            }
            if found, !inFrame { tops.append(y) }
            inFrame = found
        }
        return tops
    }

    /// The lightest pixel in a region.
    private static func brightest(
        _ image: CGImage, columns: ClosedRange<Int>, rows: ClosedRange<Int>
    ) -> Int {
        let data = bytes(image)
        var most = 0
        for y in rows where y >= 0 && y < image.height {
            for x in columns where x >= 0 && x < image.width {
                let i = (y * image.width + x) * 4
                most = max(most, (Int(data[i]) + Int(data[i + 1]) + Int(data[i + 2])) / 3)
            }
        }
        return most
    }

    /// How far the lightest and darkest pixels of a region are apart. Text on a
    /// flat background reads as a spread; a flat background alone reads as
    /// nothing.
    private static func contrast(
        _ image: CGImage, columns: ClosedRange<Int>, rows: ClosedRange<Int>
    ) -> Int {
        let data = bytes(image)
        var most = 0
        var least = 255
        for y in rows where y >= 0 && y < image.height {
            for x in columns where x >= 0 && x < image.width {
                let i = (y * image.width + x) * 4
                let grey = (Int(data[i]) + Int(data[i + 1]) + Int(data[i + 2])) / 3
                most = max(most, grey)
                least = min(least, grey)
            }
        }
        return most - least
    }

    /// How many pixels are something other than the black the render sits on.
    private static func inkedPixels(_ image: CGImage) -> Int {
        let data = bytes(image)
        var count = 0
        for i in Swift.stride(from: 0, to: data.count, by: 4) {
            if Int(data[i]) + Int(data[i + 1]) + Int(data[i + 2]) > 12 { count += 1 }
        }
        return count
    }

    /// The rightmost column with anything in it.
    private static func lastInkedColumn(_ image: CGImage) -> Int? {
        let data = bytes(image)
        for x in Swift.stride(from: image.width - 1, through: 0, by: -1) {
            for y in 0..<image.height {
                let i = (y * image.width + x) * 4
                if Int(data[i]) + Int(data[i + 1]) + Int(data[i + 2]) > 12 { return x }
            }
        }
        return nil
    }
}
