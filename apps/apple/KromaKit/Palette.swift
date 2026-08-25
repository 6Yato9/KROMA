import SwiftUI

/// A colour, as it ends up on screen.
///
/// A mirror of `pe_theme::Rgb8`: eight bits a channel and no alpha, because
/// that is what every entry in the palette is and what every ramp evaluates to.
///
/// The SwiftUI `Color` is derived from the bytes rather than written beside
/// them. One `(r, g, b)` triple per colour is the only arrangement in which the
/// number the fixture checks and the number that reaches the screen cannot
/// become two different colours.
public struct Rgb8: Equatable, Hashable, Sendable {
    public let r: UInt8
    public let g: UInt8
    public let b: UInt8

    public init(_ r: UInt8, _ g: UInt8, _ b: UInt8) {
        self.r = r
        self.g = g
        self.b = b
    }

    /// The whole of the glue between the palette and the toolkit.
    ///
    /// sRGB named explicitly. These bytes were picked off a display, so the
    /// space they are read in is part of what they mean.
    public var color: Color {
        Color(.sRGB, red: Double(r) / 255, green: Double(g) / 255, blue: Double(b) / 255)
    }
}

/// Kroma's palette. Every colour in the application comes from here.
///
/// A mirror of `crates/pe-theme/src/lib.rs`, and the same numbers the Windows
/// shell draws from. That is not tidiness: before the shared crate existed the
/// greys were written at each call site and had already drifted — the viewer
/// surround, the filmstrip and the status bar were three different shades of
/// what was meant to be one background. A second palette in Swift is that
/// mistake again, further apart and with nothing compiling both.
///
/// The scheme is Resolve's, read off the colour page, and it is built from very
/// few values: four greys for surfaces, one hairline for every division, three
/// text weights, and a single warm accent spent only on what is *active*. The
/// restraint is the part worth copying — Resolve's interface is almost entirely
/// grey, so its one orange title says where you are without having to shout. An
/// accent on every heading says nothing at all.
///
/// The cases *are* the palette, which is how this side gets the guarantee
/// `pe_theme`'s `palette!` macro gives the other. `CaseIterable` makes
/// ``allNames`` the declaration rather than a second list that could disagree
/// with it, and the exhaustive `switch` in ``rgb`` means a case cannot be
/// declared and left without a colour. Between them, a colour added here alone
/// is a colour `PaletteTests` fails on.
public enum Palette: String, CaseIterable, Sendable {

    // ---- Surfaces, darkest to lightest -----------------------------------

    /// Behind the photograph. Darkest, so nothing in the frame competes with
    /// it — a surround lighter than the picture's own shadows makes the shadows
    /// look lifted, which is a lie told to someone grading them.
    case viewer = "VIEWER"
    /// The inside of anything you type into or read a graph out of.
    case well = "WELL"
    /// Panel background — the inspector, the scopes, the filmstrip.
    case panel = "PANEL"
    /// One step up: headers, the toolbar, a hovered row.
    case raised = "RAISED"
    /// A control that sits on a panel: buttons, combo boxes, tiles.
    case control = "CONTROL"
    case controlHot = "CONTROL_HOT"

    // ---- Lines -----------------------------------------------------------

    /// Every division in the interface is this one hairline.
    case rule = "RULE"
    case boxEdge = "BOX_EDGE"
    /// The inside of the boxed number.
    case boxFill = "BOX_FILL"

    // ---- Text ------------------------------------------------------------

    case title = "TITLE"
    case label = "LABEL"
    case dim = "DIM"
    case icon = "ICON"

    // ---- Controls --------------------------------------------------------

    case track = "TRACK"
    /// How far the value has been pushed from neutral.
    case trackFill = "TRACK_FILL"
    case handle = "HANDLE"
    case handleHot = "HANDLE_HOT"
    /// Drawn around the handle so it stays legible on a coloured track.
    case handleEdge = "HANDLE_EDGE"

    /// The grid inside a plot — the curve editor, the scopes, the histogram.
    case grid = "GRID"

    // ---- The channels ----------------------------------------------------
    // Red, green and blue, wherever a channel has to be named by colour: a
    // curve trace, a parade panel, a mixer band. One set, because three
    // slightly different reds across three panels reads as three different
    // meanings to anyone who has not seen the source.

    case channelR = "CHANNEL_R"
    case channelG = "CHANNEL_G"
    case channelB = "CHANNEL_B"

    // ---- The accent ------------------------------------------------------

    /// Resolve titles the open effect in this, and spends it nowhere else.
    case accent = "ACCENT"
    /// The accent, dimmed for a fill behind text.
    case accentDim = "ACCENT_DIM"
    /// Selection. Resolve's is a muted blue, deliberately not the accent —
    /// "this is chosen" and "this is doing something" are different facts.
    case select = "SELECT"
    case warn = "WARN"
    case error = "ERROR"

    /// The bytes, and the only place they are written.
    public var rgb: Rgb8 {
        switch self {
        case .viewer: Rgb8(18, 18, 18)
        case .well: Rgb8(22, 22, 22)
        case .panel: Rgb8(33, 33, 33)
        case .raised: Rgb8(43, 43, 43)
        case .control: Rgb8(56, 56, 56)
        case .controlHot: Rgb8(70, 70, 70)

        case .rule: Rgb8(58, 58, 58)
        case .boxEdge: Rgb8(70, 70, 70)
        case .boxFill: Rgb8(20, 20, 20)

        case .title: Rgb8(228, 228, 228)
        case .label: Rgb8(176, 176, 176)
        case .dim: Rgb8(128, 128, 128)
        case .icon: Rgb8(150, 150, 150)

        case .track: Rgb8(74, 74, 74)
        case .trackFill: Rgb8(122, 122, 122)
        case .handle: Rgb8(190, 190, 190)
        case .handleHot: Rgb8(240, 240, 240)
        case .handleEdge: Rgb8(16, 16, 16)

        case .grid: Rgb8(44, 44, 44)

        case .channelR: Rgb8(226, 86, 86)
        case .channelG: Rgb8(92, 206, 110)
        case .channelB: Rgb8(104, 142, 240)

        case .accent: Rgb8(224, 106, 90)
        case .accentDim: Rgb8(70, 26, 24)
        case .select: Rgb8(46, 84, 122)
        case .warn: Rgb8(226, 168, 74)
        case .error: Rgb8(226, 96, 82)
        }
    }

    /// What a view actually paints with.
    public var color: Color { rgb.color }

    /// The three channel colours, in the order a parade draws them.
    public static let channel: [Palette] = [.channelR, .channelG, .channelB]

    // ---- What the fixture is checked against -----------------------------

    /// Every colour, by the name the engine knows it by.
    ///
    /// This and ``named(_:)`` exist for `PaletteTests`, and they are the reason
    /// a colour cannot be added on the Swift side alone: one test says every
    /// engine colour is here, the other says nothing here is absent from the
    /// engine.
    public static var allNames: [String] { allCases.map(\.rawValue) }

    /// The colour the engine calls `name`, if this side has one.
    public static func named(_ name: String) -> Rgb8? { Palette(rawValue: name)?.rgb }

    /// A colour as the fixture spells it: six upper-case hex digits, no hash.
    public static func hex(_ colour: Rgb8) -> String {
        String(format: "%02X%02X%02X", colour.r, colour.g, colour.b)
    }
}
