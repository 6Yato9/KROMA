import CoreGraphics
import SwiftUI

/// The filmstrip: a column of thumbnails down the left of the window.
///
/// **Down rather than across**, and this is the one layout decision in the
/// file. The window is wider than it is tall and a photograph is not, so a
/// horizontal strip costs height — the dimension the picture is already short
/// of. A vertical one costs width, of which there is more to spare, and it
/// holds more frames at the same cell size. `apps/windows/src/filmstrip.rs`
/// makes the same argument at the top of itself, and both shells now run the
/// strip the same way round.
///
/// **Only the cells on screen have their thumbnails asked for.** A folder of a
/// thousand photographs is a perfectly ordinary thing to open, and the
/// difference between a strip that handles it and one that does not is entirely
/// in whether it does work per photograph or per *visible* photograph. Only one
/// photograph is ever decoded — a 24-megapixel frame is 96 MB of RGBA — and the
/// whole reason a strip exists is to make a set navigable without holding it,
/// so a strip that asked for every thumbnail as the folder opened would hand
/// back with one hand the memory the design saves with the other.
///
/// The arithmetic that decides which cells those are is ``visible(from:to:stride:count:)``,
/// ``wanted(showing:count:)`` and the ``asking(top:height:count:)`` that
/// composes them — written as functions rather than inside the `GeometryReader`
/// that feeds them, because arithmetic inside a `GeometryReader` is arithmetic
/// nobody can test. That is the same reason ``SliderGeometry`` and
/// ``WheelGeometry`` exist.
///
/// The metrics and the arithmetic are `nonisolated`: they are a table of
/// constants and four pure functions, and nothing about them belongs to the
/// main actor just because the view they describe does.
public struct Filmstrip: View {
    let store: SessionStore

    public init(store: SessionStore) {
        self.store = store
    }

    public var body: some View {
        FilmstripColumn(
            entries: store.library.entries,
            current: store.library.current,
            picture: { store.thumbnail(for: $0) },
            ask: { store.requestThumbnails($0) },
            show: { store.focus($0) }
        )
    }

    // ---- what a cell costs -------------------------------------------------

    /// One cell's picture area. The Windows strip's, point for point.
    nonisolated public static let frame = CGSize(width: 104, height: 74)
    /// The line the file's name is written on, under the frame.
    nonisolated public static let caption: CGFloat = 12
    /// Between the frame and the name.
    nonisolated public static let inside: CGFloat = 2
    /// The cell's own margin — the band the current mark is drawn in, so that
    /// the mark reads as a backing behind the whole cell rather than as a
    /// border drawn on the photograph.
    nonisolated public static let pad: CGFloat = 3
    /// Between one cell and the next.
    nonisolated public static let gap: CGFloat = 6
    /// Either side of the column.
    nonisolated public static let margin: CGFloat = 6

    /// One cell, top to bottom.
    nonisolated public static var cell: CGFloat { frame.height + inside + caption + pad * 2 }

    /// Cell to cell. The number ``visible(from:to:stride:count:)`` divides by,
    /// and — because `FilmstripCells` lays a fixed-height cell out on a
    /// `LazyVStack` of exactly this spacing — the number the cells really sit
    /// on. If those two ever came apart, every range the strip asked for would
    /// be for the wrong photographs.
    nonisolated public static var stride: CGFloat { cell + gap }

    /// How wide the column is. Not resizable: a filmstrip's width is the width
    /// of a thumbnail, and a wider one has nothing more to show.
    nonisolated public static var width: CGFloat { frame.width + pad * 2 + margin * 2 }

    /// Whether a set is worth a strip.
    ///
    /// More than one photograph, which is what `apps/windows/src/main.rs`
    /// decides with `show_strip = library.len() > 1`. One photograph does not
    /// need navigating, and a session showing nothing — or the built-in chart,
    /// which is not a file and so not a set of one — has no set at all.
    nonisolated public static func isWorthShowing(count: Int) -> Bool { count > 1 }

    // ---- which cells are on screen -----------------------------------------

    /// How many cells past the edge of the view to ask for.
    ///
    /// Eight, taken from `filmstrip.rs`'s own `LOOKAHEAD`: enough that a
    /// thumbnail is usually already there by the time it scrolls into sight,
    /// few enough that opening a large folder does not queue hundreds of
    /// decodes for photographs nobody has looked at.
    nonisolated public static let lookAhead = 8

    /// Which cells a scroll offset puts on screen.
    ///
    /// Written against a bare offset rather than a direction, so it says the
    /// same thing whichever way the strip runs. It is the part that decides how
    /// much work the strip does per frame, and it is much easier to be sure of
    /// as arithmetic than by scrolling and watching.
    ///
    /// A `Range` whose lower bound is above its upper one is a crash in Swift
    /// rather than an empty range, so a strip scrolled past the end — which an
    /// elastic overscroll does momentarily — is clamped to empty here rather
    /// than handed to `ForEach` to trap on.
    nonisolated public static func visible(
        from: CGFloat, to: CGFloat, stride: CGFloat, count: Int
    ) -> Range<Int> {
        guard count > 0, stride > 0, from.isFinite, to.isFinite else { return 0..<0 }
        let ceiling = CGFloat(count)
        let first = Int(Swift.min(Swift.max((from / stride).rounded(.down), 0), ceiling))
        let last = Int(Swift.min(Swift.max((to / stride).rounded(.up), 0) + 1, ceiling))
        return first..<Swift.max(first, last)
    }

    /// What to ask the engine for, given what is on screen: the visible cells
    /// and the look-ahead past them, and never the rest of the set.
    ///
    /// Clamped to the end of the set. The engine ignores a range that runs off
    /// the end, so the clamp buys nothing at run time — it is here so that what
    /// the strip asks for is a number a test can hold to the size of the window
    /// rather than to the size of the folder.
    nonisolated public static func wanted(showing visible: Range<Int>, count: Int) -> Range<Int> {
        guard count > 0, !visible.isEmpty else { return 0..<0 }
        let first = Swift.min(visible.lowerBound, count)
        let last = Swift.min(visible.upperBound + lookAhead, count)
        return first..<Swift.max(first, last)
    }

    /// What a column scrolled to `top`, in a view `height` tall, asks for.
    ///
    /// The whole of the strip's side of the bargain with the engine, in one
    /// place. `FilmstripColumn` calls this and nothing else, so the composition
    /// — which stride, which end of the view, and whether the look-ahead is
    /// applied at all — is a thing a test can hold rather than three lines
    /// inside a `GeometryReader`.
    nonisolated public static func asking(top: CGFloat, height: CGFloat, count: Int) -> Range<Int> {
        wanted(
            showing: visible(from: top, to: top + height, stride: stride, count: count),
            count: count)
    }

    /// Which set this is, in one number.
    ///
    /// The strip asks for a *range*, and a range that has not moved is not
    /// asked for twice — which is right while somebody scrolls, and wrong the
    /// moment a second folder of the same size is opened behind it. Those
    /// photographs have never been asked for and the window is exactly where it
    /// was, so an ask keyed on the range alone would leave a strip of empty
    /// frames until somebody scrolled it.
    ///
    /// The paths and not the marks: a thumbnail arriving is not a new set, and
    /// an ask that fired on every delivery would be an ask per thumbnail.
    ///
    /// One pass over the entries, made where the set is read — `FilmstripColumn`'s
    /// body, which runs when the set changes — and never inside the
    /// `GeometryReader`, which runs on every frame of a scroll.
    nonisolated public static func identity(of entries: [LibraryEntry]) -> Int {
        var hasher = Hasher()
        for entry in entries { hasher.combine(entry.path) }
        return hasher.finalize()
    }

    /// One ask: a range, and the set it is a range into.
    public struct Request: Equatable, Sendable {
        public let set: Int
        public let range: Range<Int>

        public init(set: Int, range: Range<Int>) {
            self.set = set
            self.range = range
        }
    }

    /// The scroll view's own coordinate space, which is what turns the
    /// content's position into the offset it has been scrolled to.
    nonisolated static let space = "kroma.filmstrip"
}

/// The strip itself, drawn from values rather than from a store.
///
/// Split from ``Filmstrip`` the way `CropOverlayCanvas` is split from
/// `CropOverlay`: what the strip draws, and what it asks for, are then things a
/// test can point at without an engine, a folder of photographs and a worker
/// thread behind them.
struct FilmstripColumn: View {
    let entries: [LibraryEntry]
    /// Which one is on screen, or nil when there is no set.
    let current: Int?
    /// The picture for an entry, once its thumbnail has arrived. A closure
    /// rather than a dictionary, because the store's answer is a lookup by path
    /// and a strip asks it of every visible entry on every body evaluation.
    let picture: (LibraryEntry) -> CGImage?
    /// Ask for a range of thumbnails.
    let ask: (Range<Int>) -> Void
    /// Show a photograph. Clicking a cell is the whole point of the strip.
    let show: (Int) -> Void

    var body: some View {
        // One photograph does not need navigating. The rule lives here so that
        // the window has one thing to put in its layout rather than a condition
        // of its own that could come to disagree with this one.
        if Filmstrip.isWorthShowing(count: entries.count) {
            // Read here, once per set, and carried into the probe — see
            // ``Filmstrip/identity(of:)`` for what it is for.
            let set = Filmstrip.identity(of: entries)
            GeometryReader { outer in
                ScrollView(.vertical) {
                    FilmstripCells(
                        entries: entries, current: current, picture: picture, show: show
                    )
                    .background(probe(outer, set: set))
                }
                .coordinateSpace(.named(Filmstrip.space))
            }
            .frame(width: Filmstrip.width)
            .background(Palette.panel.color)
            // The division between the strip and the picture. `Hairline` is
            // the horizontal one; this is the same `RULE` on its side.
            .overlay(alignment: .trailing) {
                Rectangle().fill(Palette.rule.color).frame(width: 1)
            }
        }
    }

    /// Where the column has been scrolled to, turned into a range and asked
    /// for.
    ///
    /// A clear background behind the cells rather than a scroll callback,
    /// because `onScrollGeometryChange` is macOS 15 and this ships to 14. What
    /// it reads is the content's own position inside the scroll view, which is
    /// the offset with its sign flipped; everything after that is
    /// ``Filmstrip/asking(top:height:count:)``.
    ///
    /// The ask is in an `onChange` and not in the body: asking is a call into
    /// the engine, and a view body that reaches through the ABI every time
    /// SwiftUI evaluates it is a body with a side effect in it. What it watches
    /// is the range *and* the set — a range on its own would go unasked when
    /// one folder replaced another of the same size.
    private func probe(_ outer: GeometryProxy, set: Int) -> some View {
        GeometryReader { inner in
            let request = Filmstrip.Request(
                set: set,
                range: Filmstrip.asking(
                    top: -inner.frame(in: .named(Filmstrip.space)).minY,
                    height: outer.size.height,
                    count: entries.count))
            Color.clear
                .onChange(of: request, initial: true) { _, now in ask(now.range) }
        }
    }
}

/// The cells, laid out down the column.
///
/// Its own view, and not folded into the `ScrollView` above it, for two
/// reasons. The lazy stack is the thing that makes the strip cost what the
/// window costs rather than what the folder costs — only the cells on screen
/// are built at all — and a `ScrollView` renders nothing under `ImageRenderer`,
/// so this is the largest piece of the strip a headless test can be pointed at.
struct FilmstripCells: View {
    let entries: [LibraryEntry]
    let current: Int?
    let picture: (LibraryEntry) -> CGImage?
    let show: (Int) -> Void

    var body: some View {
        // The spacing and the cell's own fixed height are what
        // `Filmstrip.stride` is the sum of. Nothing here may size itself from
        // its content: a cell that grew or shrank would put the photograph at
        // an offset the arithmetic cannot name.
        LazyVStack(spacing: Filmstrip.gap) {
            ForEach(entries) { entry in
                FilmstripCell(
                    entry: entry,
                    picture: picture(entry),
                    current: entry.index == current
                )
                .onTapGesture { show(entry.index) }
            }
        }
        .padding(.horizontal, Filmstrip.margin)
    }
}

/// One photograph's cell: the frame, its name under it, and the two marks a
/// strip draws on it.
struct FilmstripCell: View {
    let entry: LibraryEntry
    let picture: CGImage?
    let current: Bool

    @State private var hovering = false

    var body: some View {
        VStack(spacing: Filmstrip.inside) {
            well
            // Under the frame, because a column of thumbnails from one shoot is
            // a column of very similar pictures and the name is often the only
            // thing that tells two of them apart. Truncated in the middle: the
            // end of a photograph's file name is where its number is.
            Text(entry.name)
                .font(.system(size: 9))
                .lineLimit(1)
                .truncationMode(.middle)
                .foregroundStyle((current ? Palette.title : Palette.dim).color)
                .frame(width: Filmstrip.frame.width, height: Filmstrip.caption)
        }
        .padding(Filmstrip.pad)
        .background(RoundedRectangle(cornerRadius: 3).fill(backing))
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .help(entry.name)
        .accessibilityLabel(entry.name)
        .accessibilityAddTraits(current ? [.isSelected] : [])
    }

    /// `SELECT` for the one on screen, and deliberately not `ACCENT`: "this is
    /// chosen" and "this is doing something" are different facts, and the
    /// accent is spent on the effect being worked in. The chosen tool in
    /// `ToolStrip` is marked the same way for the same reason.
    private var backing: Color {
        if current { return Palette.select.color }
        return hovering ? Palette.controlHot.color : Palette.panel.color
    }

    /// The frame. A fixed size whether or not there is anything to put in it —
    /// a cell that collapsed while its thumbnail was still being decoded would
    /// shuffle every cell below it up the column and back down again, one at a
    /// time, as a folder came in off the worker thread.
    private var well: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 2).fill(Palette.well.color)
            if let picture {
                // Fit rather than fill: a filmstrip is for recognising a frame,
                // and cropping the frame to a common shape is a strange way to
                // go about that.
                Image(picture, scale: 1, label: Text(entry.name))
                    .resizable()
                    .interpolation(.high)
                    .aspectRatio(contentMode: .fit)
                    .padding(2)
            } else {
                // Deliberately quiet. A strip of spinners on a folder of a
                // thousand is a light show, not information.
                Text(entry.failed ? "unreadable" : "…")
                    .font(.system(size: entry.failed ? 9 : 12))
                    .foregroundStyle(Palette.dim.color)
            }
        }
        .frame(width: Filmstrip.frame.width, height: Filmstrip.frame.height)
        .overlay(alignment: .topTrailing) {
            if entry.edited { edited }
        }
    }

    /// The mark on a photograph that has an edit parked on it, so a set half
    /// way through a pass is readable at a glance.
    ///
    /// A light dot on a dark disc, which is the shape `filmstrip.rs` draws for
    /// the same mark: the disc is what keeps it legible over a bright frame.
    /// `TITLE` on `VIEWER` rather than a colour, because the two colours in
    /// this scheme that carry meaning are already spoken for — a mark that
    /// borrowed `SELECT` would be saying that a photograph with a parked edit
    /// is the photograph on screen, and one that borrowed `ACCENT` would spend
    /// the scheme's one loud colour on something that is not active.
    private var edited: some View {
        ZStack {
            Circle().fill(Palette.viewer.color).frame(width: 9, height: 9)
            Circle().fill(Palette.title.color).frame(width: 5, height: 5)
        }
        .padding(4)
    }
}
