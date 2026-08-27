import SwiftUI

/// The inspector's four tabs.
///
/// A mirror of `pe_effects::Tab`, and checked against it by `theme.json`. The
/// Windows shell's tab row, tab for tab: this shell drew eight icons for a
/// while, which was the five collapsing sections of the Colour tab promoted to
/// peers of the other three.
public enum Tab: String, CaseIterable, Sendable {
    case colour = "Colour"
    case effects = "Effects"
    case image = "Image"
    case file = "File"

    /// What the tab says it is — the label under the glyph, the tooltip, and
    /// the accessibility name.
    ///
    /// The raw value, so the string stored in preferences, the string the
    /// fixture checks and the string the reader sees cannot become three
    /// different strings.
    public var name: String { rawValue }

    /// The SF Symbol above the label.
    ///
    /// The Windows shell draws its four by hand for parity with Resolve's —
    /// a colour wheel, a wand throwing sparks, a frame with a horizon in it,
    /// and a sheet with a folded corner. This is the one place the two shells
    /// are allowed to differ: SF Symbols are the platform idiom, they scale
    /// with the type, and a hand-drawn glyph here would be reproducing a
    /// Windows workaround rather than a design.
    ///
    /// **A name the system does not have renders as nothing** — a blank tab is
    /// one nobody can identify — so every one of these is asked of `NSImage`
    /// in `TabRowTests` rather than assumed. All four are SF Symbols 1.0–2.0,
    /// well below the 14.0 deployment target.
    public var symbol: String {
        switch self {
        case .colour: "circle.lefthalf.filled"
        case .effects: "wand.and.stars"
        case .image: "photo"
        case .file: "doc"
        }
    }

    /// Whether the viewer shows the **enclosing** frame — the whole
    /// straightened source — rather than the cropped result while this tab is
    /// open.
    ///
    /// One rule, read twice: it is what puts `CropOverlay` over the picture and
    /// what is handed to `SessionStore.setCropping`. `pe_effects::Tab`'s own
    /// answer, so the two shells cannot disagree — a rectangle drawn over a
    /// viewer that is showing the crop rather than the frame around it is a
    /// rectangle with nothing outside it to drag back in, and cropping left on
    /// after switching away shows the wrong picture on every other tab.
    public var showsWholeFrame: Bool { self == .image }

    /// Which of a document's rows the Effects tab puts on screen: the ones the
    /// user added, in the document's own order.
    ///
    /// The index is carried because a stack row's reorder arrows are positions
    /// in the *document*, not in whatever subset is on screen — numbering a
    /// filtered list from zero moves the wrong row. The row's own id is the
    /// identity, so a removal renumbers nothing.
    public static func added(_ rows: [Snapshot.Row]) -> [Drawn] {
        rows.enumerated()
            .filter { !$0.element.pinned }
            .map { Drawn(index: $0.offset, row: $0.element) }
    }
}

/// One collapsing section of the Colour tab.
///
/// A mirror of `pe_effects::Section`. The order is `main.rs`'s and neither
/// alphabetical nor historical: Curves first, because the curve carries the
/// histogram and a histogram belongs at the top.
public enum Section: String, CaseIterable, Sendable {
    case curves = "Curves"
    case basic = "Basic"
    case colourWarper = "Colour Warper"
    /// Resolve's own label, hyphen and American spelling included. A proper
    /// noun here rather than a description, which is why it is not tidied into
    /// the spelling the rest of the application uses.
    case colourWheels = "Primaries - Color Wheels"
    case colourMixer = "Colour Mixer"

    public var title: String { rawValue }

    /// The pinned effects this section draws, in the order it draws them.
    ///
    /// `pe_effects::Section::effects`, key for key, and `TabRowTests` fails if
    /// the two lists ever disagree.
    public var effects: [String] {
        switch self {
        case .curves: ["curves"]
        case .basic:
            ["white_balance", "exposure", "contrast", "tone", "presence", "colour"]
        case .colourWarper: ["colour_warper"]
        case .colourWheels: ["primaries", "log_wheels"]
        case .colourMixer: ["colour_mixer"]
        }
    }

    /// Whether the section is open the first time it is seen.
    ///
    /// The warper and the mixer are shut, and both because each is large and
    /// occasional — a grid and six wheels, not a few rows. A tab that opens
    /// everything is the scrolling problem the tabs exist to solve. Only the
    /// first time: what the reader folds is remembered.
    public var startsOpen: Bool {
        self != .colourWarper && self != .colourMixer
    }

    /// The section that draws an effect, if a pinned one does.
    ///
    /// `nil` is the honest answer for an added effect: the Effects tab draws it
    /// as one of the stack's rows, not as a fixed panel of its own.
    public static func of(_ effect: String) -> Section? {
        allCases.first { $0.effects.contains(effect) }
    }

    /// One row a section draws, and where it sits in the whole stack.
    public struct Drawn: Identifiable, Sendable {
        public let index: Int
        public let row: Snapshot.Row

        public var id: UInt64 { row.id }
    }

    /// Which of a document's rows this section puts on screen, in the order it
    /// draws them.
    ///
    /// One function rather than a rule in the view and a copy of it in a test,
    /// because the property that matters is about *all five sections and the
    /// Effects tab at once*: every row belongs to exactly one of them. A row
    /// belonging to none is drawn nowhere at all, with nothing to say so.
    ///
    /// The section's own effect order, not the stack's — a section decides
    /// which panels belong together and in what sequence. A key the document
    /// does not carry draws nothing, which is what an engine that has stopped
    /// pinning a row looks like. And the pinned row is matched, never an added
    /// copy of the same effect: a document may well carry a second exposure.
    public func draws(_ rows: [Snapshot.Row]) -> [Drawn] {
        effects.compactMap { key in
            rows.enumerated()
                .first { $0.element.pinned && $0.element.effect == key }
                .map { Drawn(index: $0.offset, row: $0.element) }
        }
    }
}

extension Tab {
    /// The Effects tab hands back the same shape a section does.
    public typealias Drawn = Section.Drawn
}

/// The tab row: four cells, one page on screen at a time.
///
/// `tab_row` in `main.rs`, measurement for measurement — a glyph above a
/// label in each of four equal cells, the chosen one carrying an **`ACCENT`**
/// underline along its bottom edge.
///
/// The accent belongs here. The eight-icon strip this replaces argued the
/// opposite and used `SELECT`, but that was this shell's own choice and not the
/// Windows one; the two are meant to match.
///
/// `RAISED` behind it and a `RULE` hairline under it, because it is a header
/// and every other header in the application is drawn that way.
public struct TabRow: View {
    @Binding var chosen: Tab

    public init(chosen: Binding<Tab>) {
        self._chosen = chosen
    }

    /// What a tab's glyph and label are drawn in: `main.rs`'s three tints.
    ///
    /// A static so the rule can be asserted without standing a view up. The
    /// **accent** is not one of them — it is the underline under the chosen
    /// tab, and that is the one place in this row it is spent.
    static func tint(chosen: Bool, hovering: Bool) -> Color {
        if chosen { return Palette.title.color }
        return (hovering ? Palette.handle : Palette.label).color
    }

    /// Tall enough for a glyph with a label under it. `main.rs`'s 44.
    static let height: CGFloat = 44
    /// How far in from each end the underline stops. `main.rs`'s 10.
    static let underlineInset: CGFloat = 10
    static let underline: CGFloat = 2

    public var body: some View {
        HStack(spacing: 0) {
            ForEach(Tab.allCases, id: \.self) { tab in
                TabCell(tab: tab, chosen: tab == chosen) { chosen = tab }
            }
        }
        .frame(height: Self.height)
        .frame(maxWidth: .infinity)
        .background(Palette.raised.color)
        .overlay(alignment: .bottom) { Hairline() }
    }

    /// One tab. `TITLE` when it is the one showing, `HANDLE` under the
    /// pointer, `LABEL` otherwise — `main.rs`'s three tints.
    private struct TabCell: View {
        let tab: Tab
        let chosen: Bool
        let pick: () -> Void

        @State private var hovering = false

        var body: some View {
            Button(action: pick) {
                VStack(spacing: 3) {
                    Image(systemName: tab.symbol)
                        .imageScale(.medium)
                    Text(tab.name)
                        .font(.system(size: 10.5))
                }
                .foregroundStyle(tint)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .contentShape(Rectangle())
                .overlay(alignment: .bottom) {
                    // Drawn always and made invisible, rather than added and
                    // removed: a bar that appears changes the cell's height on
                    // every click, and the row jumps.
                    Rectangle()
                        .fill(Palette.accent.color)
                        .frame(height: TabRow.underline)
                        .padding(.horizontal, TabRow.underlineInset)
                        .opacity(chosen ? 1 : 0)
                }
            }
            .buttonStyle(.plain)
            .onHover { hovering = $0 }
            // The name twice, and both are wanted: the tooltip is for the
            // reader who cannot place the glyph, the label for the one who
            // never looks at it.
            .help(tab.name)
            .accessibilityLabel(tab.name)
        }

        private var tint: Color { TabRow.tint(chosen: chosen, hovering: hovering) }
    }
}
