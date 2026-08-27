import CoreGraphics
import SwiftUI

/// The crop tool's panel: the straightening angle, the quarter-turns and the
/// flips, the aspect lock, and a way back to the whole photograph.
///
/// The one panel in the inspector that is not generated from the registry.
/// There is no `Effect` behind it and no parameters to look up — it edits the
/// document's *geometry*, which is one value on the document rather than a row
/// in its stack. `apps/windows`'s Image page is the same four groups in the
/// same order, minus the Zoom, Position and edge readouts that are the crop
/// rectangle said a second way; here the rectangle is dragged on the picture,
/// which is where `CropOverlay` already puts it.
///
/// **Everything drawn here is the value the engine gave back.** Each control
/// proposes a geometry through ``SessionStore/setGeometry(_:)`` and then reads
/// ``SessionStore/geometry``, which is what the document now holds — not what
/// was asked for. The engine corrects: quarter-turns are taken modulo four, a
/// locked aspect re-shapes the crop, and the crop is slid, then shrunk, back
/// inside the straightened source. A panel that displayed its own proposal
/// would disagree with the overlay drawn next to it, and would jump to the real
/// value the moment anything else refreshed.
public struct CropPanel: View {
    let store: SessionStore

    public init(store: SessionStore) {
        self.store = store
    }

    // ---- the aspect presets ----------------------------------------------

    /// The locks the panel offers, in `crop.rs`'s order.
    ///
    /// Free and Original first because they are the two that are not a number,
    /// then the ratios people ask for by name.
    public static let aspects: [(name: String, lock: AspectLock)] = [
        ("Free", .free),
        ("Original", .original),
        ("1:1", .ratio(w: 1, h: 1)),
        ("3:2", .ratio(w: 3, h: 2)),
        ("4:3", .ratio(w: 4, h: 3)),
        ("16:9", .ratio(w: 16, h: 9)),
        ("5:4", .ratio(w: 5, h: 4)),
    ]

    /// What a lock is called.
    ///
    /// Matched on the *ratio* and not on the two numbers, because a lock that
    /// has been through the drag path has lost its spelling: the ABI carries
    /// one float, so 16:9 goes out as 1.777… and comes back as
    /// `.ratio(w: 1.777…, h: 1)`. The same lock, and the menu has to go on
    /// saying "16:9" about it rather than falling through to a number nobody
    /// chose.
    ///
    /// A ratio that is none of the presets is printed rather than dropped: a
    /// document written by a later version, or by the Windows shell, may hold
    /// one, and a menu showing "Free" for a crop that is locked would be a lie
    /// about what the next drag will do.
    public static func name(of aspect: AspectLock) -> String {
        switch aspect {
        case .free: return "Free"
        case .original: return "Original"
        case .ratio:
            guard let want = aspect.widthOverHeight else { return "Free" }
            for preset in aspects {
                if let have = preset.lock.widthOverHeight, abs(have - want) < 1e-4 {
                    return preset.name
                }
            }
            return String(format: "%.2f:1", want)
        }
    }

    /// The lock a menu entry names, or nil for a name the panel never wrote —
    /// which is what a printed ratio above comes back as, and is why choosing
    /// one is a no-op rather than a reset to Free.
    public static func lock(named name: String) -> AspectLock? {
        aspects.first { $0.name == name }?.lock
    }

    /// The names the menu lists, plus whatever the document is holding if that
    /// is not one of them — a `ChoiceMenu` whose current value is absent from
    /// its own options shows a tick against nothing.
    static func options(showing aspect: AspectLock) -> [String] {
        let names = aspects.map(\.name)
        let current = name(of: aspect)
        return names.contains(current) ? names : names + [current]
    }

    // ---- the straightening angle -----------------------------------------

    /// Lightroom's Angle slider, and `crop.rs`'s Rotation Angle: forty-five
    /// degrees either way, and zero is where it does nothing. Past forty-five a
    /// straighten is a quarter-turn, which is the row below.
    public static let angle = Bounds(min: -45, max: 45, default: 0, neutral: 0)

    // ---- the glyphs -------------------------------------------------------

    /// Anticlockwise and clockwise quarter-turns, and the two flips.
    ///
    /// **A name the system does not have renders as nothing**, exactly as in
    /// `TabRow`, and a blank button on a row of four is one nobody can
    /// identify — so `CropPanelTests` asks `NSImage` for each of these rather
    /// than assuming. `rotate.left` and `rotate.right` are the drawings that
    /// say "quarter-turn" most plainly and are **not** used: they are SF
    /// Symbols 6, which is macOS 15, and the deployment target here is 14.
    /// These four are all SF Symbols 1 and 2.
    public static let turnAnticlockwise = "arrow.counterclockwise"
    public static let turnClockwise = "arrow.clockwise"
    public static let flipHorizontally =
        "arrow.left.and.right.righttriangle.left.righttriangle.right"
    public static let flipVertically =
        "arrow.up.and.down.righttriangle.up.righttriangle.down"

    /// Every glyph the panel draws, for the test that asks whether they exist.
    public static let symbols = [
        turnAnticlockwise, turnClockwise, flipHorizontally, flipVertically,
    ]

    // ---- what gets proposed ----------------------------------------------

    /// One field of a geometry replaced, and the rest carried over.
    ///
    /// A free function so what each control asks for can be checked without
    /// standing a view up: the interesting half of this panel is the difference
    /// between what it proposes and what it draws, and that is only visible if
    /// the proposal can be built on its own.
    public static func proposed(
        from geometry: GeometryValue,
        angle: Double? = nil,
        turns: Int? = nil,
        flipH: Bool? = nil,
        flipV: Bool? = nil,
        aspect: AspectLock? = nil
    ) -> GeometryValue {
        GeometryValue(
            centre: geometry.centre,
            size: geometry.size,
            angle: angle ?? geometry.angle,
            turns: turns ?? geometry.turns,
            flipH: flipH ?? geometry.flipH,
            flipV: flipV ?? geometry.flipV,
            aspect: aspect ?? geometry.aspect
        )
    }

    /// Propose a change against whatever the engine last stored.
    ///
    /// `store.geometry` and not a copy held here: mid-drag it is the corrected
    /// answer to the previous frame's proposal, so a straighten drag builds
    /// each frame on the crop the engine actually kept.
    private func propose(
        angle: Double? = nil,
        turns: Int? = nil,
        flipH: Bool? = nil,
        flipV: Bool? = nil,
        aspect: AspectLock? = nil
    ) {
        store.setGeometry(
            Self.proposed(
                from: store.geometry, angle: angle, turns: turns,
                flipH: flipH, flipV: flipV, aspect: aspect))
    }

    // ---- the panel --------------------------------------------------------

    public var body: some View {
        let geometry = store.geometry
        return VStack(alignment: .leading, spacing: 2) {
            straighten(geometry)
            orientation(geometry)
            aspectRow(geometry)
            resetRow
        }
        .padding(.vertical, 6)
    }

    /// The angle, as a `ScalarRow`, so it is the same control as the thirty
    /// above it in every other tool.
    ///
    /// The value handed in is the engine's. It is the one field of a geometry
    /// that comes back exactly as proposed — turns are taken modulo four and
    /// the lock re-shapes the crop, but nothing clamps a straighten — so what
    /// the correction moves here is the *crop*, and `CropOverlay` is what draws
    /// that.
    private func straighten(_ geometry: GeometryValue) -> some View {
        ScalarRow(
            name: "Straighten", unit: "°", value: Float(geometry.angle),
            bounds: Self.angle, isActive: true,
            onChange: { propose(angle: Double($0)) },
            onBegin: { store.beginInteraction("Straighten") },
            onEnd: { store.endInteraction() }
        )
    }

    /// The quarter-turns and the flips, on one row — `crop.rs` puts the same
    /// four side by side, under the angle they follow.
    ///
    /// The turns are buttons because a quarter-turn is a step, and the flips
    /// are chips because a flip is a state you are in. Anticlockwise asks for
    /// one *less* than the document holds: the engine takes the count modulo
    /// four, so minus one arrives as three, and reading the answer back is what
    /// keeps this row from ever showing a negative turn.
    private func orientation(_ geometry: GeometryValue) -> some View {
        HStack(spacing: RowMetrics.gap) {
            label("Orientation")
            turn(Self.turnAnticlockwise, "Rotate anticlockwise", to: geometry.turns - 1)
            turn(Self.turnClockwise, "Rotate clockwise", to: geometry.turns + 1)
            flip(
                Self.flipHorizontally, "Flip horizontally", on: geometry.flipH,
                set: { propose(flipH: $0) })
            flip(
                Self.flipVertically, "Flip vertically", on: geometry.flipV,
                set: { propose(flipV: $0) })
            Spacer(minLength: 0)
        }
        .frame(height: RowMetrics.height)
    }

    private func turn(_ symbol: String, _ what: String, to turns: Int) -> some View {
        Button {
            propose(turns: turns)
        } label: {
            Image(systemName: symbol).imageScale(.small)
        }
        .buttonStyle(KromaButtonStyle())
        .help(what)
        .accessibilityLabel(what)
    }

    private func flip(
        _ symbol: String, _ what: String, on: Bool, set: @escaping (Bool) -> Void
    ) -> some View {
        Toggle(isOn: Binding(get: { on }, set: set)) {
            Image(systemName: symbol).imageScale(.small)
        }
        .toggleStyle(KromaToggleButtonStyle())
        .help(what)
        .accessibilityLabel(what)
    }

    /// What the next drag has to hold.
    ///
    /// A menu and not a row of chips: there are seven of these and
    /// ``ChoiceChips`` is for a short set — seven of them do not wrap, so in a
    /// panel this wide the last three would be drawn off the end of it.
    ///
    /// Choosing one is an edit, because the engine applies the lock to the crop
    /// there and then rather than waiting for a drag — which is why the size
    /// that comes back is usually not the size that went in, and why the
    /// overlay moves when this changes.
    private func aspectRow(_ geometry: GeometryValue) -> some View {
        HStack(spacing: RowMetrics.gap) {
            label("Aspect")
            ChoiceMenu(
                options: Self.options(showing: geometry.aspect),
                chosen: Self.name(of: geometry.aspect)
            ) { name in
                if let lock = Self.lock(named: name) { propose(aspect: lock) }
            }
            .frame(maxWidth: 132)
            Spacer(minLength: 0)
        }
        .frame(height: RowMetrics.height)
    }

    /// The whole photograph again — crop, angle, turns, flips and lock.
    ///
    /// ``SessionStore/resetGeometry()`` rather than a proposal of the identity:
    /// there is nothing for the engine to correct about "all of it", so this is
    /// the one control here with no answer to read back.
    private var resetRow: some View {
        HStack(spacing: RowMetrics.gap) {
            label("")
            Button("Reset Crop") { store.resetGeometry() }
                .buttonStyle(KromaButtonStyle())
                .help("The whole photograph, unturned and unflipped")
            Spacer(minLength: 0)
        }
        .frame(height: RowMetrics.height)
    }

    /// The label column, drawn exactly as ``ScalarRow`` draws its own, so the
    /// four rows line up. Getting this right once is most of what makes a panel
    /// look like the rest of the application.
    private func label(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 11.5))
            .foregroundStyle(Palette.label.color)
            .frame(width: RowMetrics.label, alignment: .trailing)
            .lineLimit(1)
            .truncationMode(.tail)
    }
}
