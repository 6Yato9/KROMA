import SwiftUI

/// The colour page's tools, and the strip that chooses between them.
///
/// A mirror of `pe_effects::Tool`, and checked against it by the fixture: the
/// same seven tools in the same order, each claiming the same effects. Which
/// tool draws an effect is one answer per effect, so it is decided in the
/// engine and read here rather than decided twice.
///
/// The strip exists because eleven pinned panels in one scrolling column is
/// the arrangement Resolve's colour page was built to avoid — reaching the
/// warper meant scrolling past a hundred and thirty controls, and every title
/// on the way wore the accent at once.
public enum Tool: String, CaseIterable, Sendable {
    case basic = "Basic"
    case colourWheels = "Colour Wheels"
    case curves = "Curves"
    case colourWarper = "Colour Warper"
    case colourMixer = "Colour Mixer"
    /// Whatever the user added, and the browser that adds more.
    case effects = "Effects"
    /// The crop, the straightening angle, the quarter-turns and the flips.
    ///
    /// The one tool that edits the document's *geometry* rather than a row in
    /// its stack — so it owns no pinned effects, there is no `Effect` behind it
    /// and no registry parameters to generate controls from. `CropPanel` is
    /// what it draws.
    case crop = "Crop"

    /// What the button says it is — the tooltip, and the accessibility label.
    ///
    /// The raw value, so the string stored in preferences, the string the
    /// fixture checks and the string the reader sees cannot become three
    /// different strings.
    public var name: String { rawValue }

    /// The pinned effects this tool draws, in the order it draws them.
    ///
    /// `pe_effects::Tool::effects`, key for key, and
    /// `ToolStripTests` fails if the two lists ever disagree. The six under
    /// Basic are the Lightroom-ish ones the Windows shell already gathers
    /// under a single header, in `PINNED_ROWS` order: neutralise the light,
    /// set the exposure, then shape the tone, the detail, and the colour.
    public var effects: [String] {
        switch self {
        case .basic:
            ["white_balance", "exposure", "contrast", "tone", "presence", "colour"]
        case .colourWheels: ["primaries", "log_wheels"]
        case .curves: ["curves"]
        case .colourWarper: ["colour_warper"]
        case .colourMixer: ["colour_mixer"]
        // Neither of these is pinned, and deliberately. Effects shows the
        // rows the user put there; Crop edits the document's geometry, which is
        // not a row at all.
        case .effects: []
        case .crop: []
        }
    }

    /// The SF Symbol on the tool's button.
    ///
    /// The Windows shell draws its glyphs by hand for parity with Resolve's;
    /// this is the one place the two shells are allowed to differ. SF Symbols
    /// are the platform idiom, they scale with the type, and a hand-drawn
    /// glyph here would be reproducing a Windows workaround rather than a
    /// design.
    ///
    /// **A name the system does not have renders as nothing** — a blank button
    /// in a strip of seven is one nobody can identify — so every one of these
    /// is asked of `NSImage` in `ToolStripTests` rather than assumed. All seven
    /// are from 2019–2021, well below the 14.0 deployment target: `crop` is
    /// SF Symbols 1.0, and was the first name tried.
    public var symbol: String {
        switch self {
        case .basic: "slider.horizontal.3"
        case .colourWheels: "circle.lefthalf.filled"
        case .curves: "point.topleft.down.curvedto.point.bottomright.up"
        case .colourWarper: "square.grid.3x3"
        case .colourMixer: "paintpalette"
        case .effects: "wand.and.stars"
        case .crop: "crop"
        }
    }

    /// Whether the viewer shows the **enclosing** frame — the whole
    /// straightened source — rather than the cropped result while this tool is
    /// open.
    ///
    /// One rule, read twice: it is what puts `CropOverlay` over the picture and
    /// what is handed to `SessionStore.setCropping`. Written here rather than
    /// as two comparisons in `ContentView` because the two must not be able to
    /// disagree — a rectangle drawn over a viewer that is showing the crop
    /// rather than the frame around it is a rectangle with nothing outside it
    /// to drag back in, and cropping left on after switching away shows the
    /// wrong picture in every other tool.
    public var showsWholeFrame: Bool { self == .crop }

    /// The tool that draws an effect, if a pinned one does.
    ///
    /// `nil` is the honest answer for an added effect: ``Tool/effects`` draws
    /// it as one of the stack's rows, not as a fixed panel of its own.
    public static func of(_ effect: String) -> Tool? {
        allCases.first { $0.effects.contains(effect) }
    }

    /// One row a tool draws, and where it sits in the whole stack.
    ///
    /// The index is carried because a stack row's reorder arrows are
    /// positions in the *document*, not in whatever subset is on screen —
    /// numbering a filtered list from zero moves the wrong row. The row's own
    /// id is the identity, so a removal renumbers nothing.
    public struct Drawn: Identifiable, Sendable {
        public let index: Int
        public let row: Snapshot.Row

        public var id: UInt64 { row.id }
    }

    /// Which of a document's rows this tool puts on screen, in the order it
    /// draws them.
    ///
    /// One function rather than a rule in the view and a copy of it in a test,
    /// because the property that matters is about *all seven tools at once*:
    /// every row belongs to exactly one of them. A row belonging to none is
    /// drawn nowhere at all, with nothing to say so.
    ///
    /// A tool that claims nothing draws nothing, which is the right answer for
    /// Crop: its panel is not made of rows.
    ///
    /// A pinned tool follows its own effect order — the tool decides which
    /// panels belong together and in what sequence, not the stack. A key the
    /// document does not carry draws nothing, which is what an engine that has
    /// stopped pinning a row looks like. And the pinned row is matched, never
    /// an added copy of the same effect: a document may well carry a second
    /// exposure.
    public func draws(_ rows: [Snapshot.Row]) -> [Drawn] {
        switch self {
        case .effects:
            return rows.enumerated()
                .filter { !$0.element.pinned }
                .map { Drawn(index: $0.offset, row: $0.element) }
        default:
            return effects.compactMap { key in
                rows.enumerated()
                    .first { $0.element.pinned && $0.element.effect == key }
                    .map { Drawn(index: $0.offset, row: $0.element) }
            }
        }
    }
}

/// The strip: seven buttons, one tool on screen at a time.
///
/// **The accent is not here.** A selected tool is "this is chosen", which is
/// `SELECT` — the same colour the scopes toggle and the choice chips use. The
/// accent stays on the name of the effect being worked in, and keeping the two
/// apart is the whole reason the scheme carries both.
///
/// `RAISED` behind it and a `RULE` hairline under it, because it is a header
/// and every other header in the application is drawn that way.
public struct ToolStrip: View {
    @Binding var chosen: Tool

    public init(chosen: Binding<Tool>) {
        self._chosen = chosen
    }

    public var body: some View {
        HStack(spacing: 2) {
            ForEach(Tool.allCases, id: \.self) { tool in
                ToolButton(tool: tool, chosen: tool == chosen) { chosen = tool }
            }
        }
        .padding(.horizontal, 4)
        .padding(.vertical, 4)
        .frame(maxWidth: .infinity)
        .background(Palette.raised.color)
        .overlay(alignment: .bottom) { Hairline() }
    }

    /// One tool's button. `ICON` on the strip at rest, `TITLE` on `SELECT`
    /// when it is the one showing.
    private struct ToolButton: View {
        let tool: Tool
        let chosen: Bool
        let pick: () -> Void

        @State private var hovering = false

        var body: some View {
            Button(action: pick) {
                Image(systemName: tool.symbol)
                    .imageScale(.large)
                    .foregroundStyle((chosen ? Palette.title : Palette.icon).color)
                    .frame(maxWidth: .infinity)
                    .frame(height: Self.height)
                    .background(RoundedRectangle(cornerRadius: 2).fill(fill))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onHover { hovering = $0 }
            // The name twice, and both are wanted: the tooltip is for the
            // reader who cannot place the glyph, the label for the one who
            // never sees it.
            .help(tool.name)
            .accessibilityLabel(tool.name)
        }

        /// Tall enough for a large symbol with a little air, and a round number
        /// so the whole row of them reads as one band.
        static let height: CGFloat = 26

        private var fill: Color {
            if chosen { return Palette.select.color }
            return hovering ? Palette.controlHot.color : Palette.raised.color
        }
    }
}
