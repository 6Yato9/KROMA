import CoreGraphics
import SwiftUI

// -----------------------------------------------------------------------------
// The three ways of comparing
// -----------------------------------------------------------------------------

/// Hold the graded picture up against the ungraded one, or stop.
///
/// `main.rs`'s `Compare`, and the same argument decides the shapes. A wipe is
/// for "did that move go too far" — the eye reads a discontinuity across a seam
/// far more finely than it reads two pictures a hand's width apart. Side by
/// side is for "which of these do I prefer", where a seam would fuse the two
/// into one image and stop you seeing either.
///
/// The engine composites the two *pictures* — it owns the textures and presents
/// to the layer itself. Everything in this file is the chrome over the top.
public enum Compare: CaseIterable, Sendable, Equatable {
    /// The default, and the one the cycling button starts and returns to.
    case off
    /// One picture with a seam: ungraded to the left of it, graded to the right.
    case wipe
    /// Two half-size pictures with a real gap: ungraded left, graded right.
    case side

    /// The next mode round, so one button can be the whole control.
    ///
    /// Off is in the cycle rather than being a separate way out: a comparison
    /// you cannot turn off with the button that turned it on is a control that
    /// only works in one direction.
    public var next: Compare {
        switch self {
        case .off: .wipe
        case .wipe: .side
        case .side: .off
        }
    }

    /// What the button says, as the Windows one does — the mode is in the
    /// label rather than in a state the reader has to remember.
    public var label: String {
        switch self {
        case .off: "Compare"
        case .wipe: "Compare · Wipe"
        case .side: "Compare · Side"
        }
    }

    /// Whether anything is being compared at all.
    public var on: Bool { self != .off }
}

// -----------------------------------------------------------------------------
// Where the chrome goes
// -----------------------------------------------------------------------------

/// The arithmetic of a comparison: where the seam is, and where the two halves
/// of a side by side sit.
///
/// Separate from the view because it is the one part of a comparison this side
/// decides, and the only part of it a machine can check. It has to agree with
/// `Session::composite`, which is what actually draws the two pictures — a seam
/// painted anywhere but over the engine's own discontinuity is worse than no
/// seam at all, because it says the picture changes somewhere it does not.
public enum CompareGeometry {

    /// How near the seam a press has to be to take hold of it, in points.
    /// `main.rs` uses twenty-four, which is a comfortable target for a line
    /// one and a half points wide.
    public static let grab: CGFloat = 24

    /// The gap between the two halves of a side by side, in **device pixels**:
    /// `session.rs`'s `SIDE_GAP`. In pixels because that is the unit the engine
    /// works the halves out in, and a gap that changed width with the display's
    /// scale would not be the gap the engine left.
    public static let gap: CGFloat = 8

    /// Where the seam lands across a viewer `width` points wide.
    ///
    /// The engine draws the ungraded frame through a scissor rectangle
    /// `round(wipe * width)` pixels wide from the left of the *target*, so the
    /// discontinuity is at that fraction of the whole viewer — not of a picture
    /// rectangle inside it, which is what the Windows shell measures against
    /// because egui composites the two pictures at the size it drew them.
    ///
    /// Clamped, because the fraction the engine stored is clamped: dragging
    /// past the edge of a window is how a user asks for either end.
    public static func seam(wipe: CGFloat, width: CGFloat) -> CGFloat {
        min(max(wipe, 0), 1) * width
    }

    /// And the same arithmetic backwards: the fraction a pointer at `x` is
    /// asking for. What a drag on the seam sends to the engine.
    public static func fraction(ofX x: CGFloat, width: CGFloat) -> CGFloat {
        guard width > 0 else { return 0 }
        return min(max(x / width, 0), 1)
    }

    /// Whether a press at `x` has hold of the seam.
    public static func grabsSeam(_ x: CGFloat, wipe: CGFloat, width: CGFloat) -> Bool {
        abs(x - seam(wipe: wipe, width: width)) <= grab
    }

    /// Where the two half-size pictures sit — `session.rs`'s `side_rects`,
    /// in the viewer's own points.
    ///
    /// Half the width each less the gap, with the height brought down by the
    /// same factor so neither picture is stretched, and both centred
    /// vertically. `scale` is the display's, because the gap the engine leaves
    /// is a number of pixels and this side is working in points.
    ///
    /// The gap is bounded by a quarter of the width for the engine's reason: a
    /// very small viewer should still get two pictures rather than a gap with
    /// slivers either side.
    public static func halves(in size: CGSize, scale: CGFloat) -> (
        before: CGRect, after: CGRect
    ) {
        guard size.width > 0, size.height > 0 else { return (.zero, .zero) }
        let gap = min(Self.gap / max(scale, 1), size.width / 4)
        let w = (size.width - gap) / 2
        let h = size.height * w / size.width
        let y = (size.height - h) / 2
        return (
            CGRect(x: 0, y: y, width: w, height: h),
            CGRect(x: size.width - w, y: y, width: w, height: h)
        )
    }
}

// -----------------------------------------------------------------------------
// The overlay
// -----------------------------------------------------------------------------

/// The seam and the labels, over the two pictures the engine composited.
///
/// **Nothing here takes the pointer.** The whole layer is
/// `allowsHitTesting(false)`, and the drag that moves the seam lives in
/// `MetalViewerView` with the zoom and the pan — see `ViewerDrag`. A SwiftUI
/// layer in front of the viewer claims the AppKit hit test for the *hosting
/// view*, which is `MetalViewerView`'s ancestor rather than anything below it
/// in the responder chain, so every scroll and every press the overlay covers
/// simply stops. `MetalViewerTests` measures that, because it is invisible
/// until somebody reaches for the wheel.
public struct CompareOverlay: View {
    let store: SessionStore

    @Environment(\.displayScale) private var scale

    public init(store: SessionStore) {
        self.store = store
    }

    public var body: some View {
        CompareOverlayCanvas(mode: store.compare, wipe: store.wipe, scale: scale)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

/// Everything a comparison paints, given the mode and the seam.
///
/// Split from `CompareOverlay` so it can be rendered without an engine, the way
/// `CropOverlayCanvas` is: where a seam lands and what a label says are exactly
/// the kind of thing that stays wrong for weeks because nothing can fail on it.
///
/// The colours are absolute rather than from the palette, and deliberately —
/// the same decision `CropOverlayCanvas` records. A hairline across a
/// photograph and a dark plate under a caption on it are pictures of something,
/// not interface furniture, and `draw_compare` paints the same four values.
struct CompareOverlayCanvas: View {
    let mode: Compare
    /// Where the seam sits, as a fraction of the viewer's width.
    let wipe: CGFloat
    /// The display's scale, which the gap in a side by side is measured in.
    let scale: CGFloat

    /// How far a label sits in from the corner it is anchored to. Eight for a
    /// wipe, six for a side by side, which is `draw_compare`'s pair — the
    /// halves are smaller pictures and the caption sits closer in on them.
    static let wipeInset: CGFloat = 8
    static let sideInset: CGFloat = 6

    var body: some View {
        Canvas { context, size in
            switch mode {
            case .off:
                return
            case .wipe:
                let x = CompareGeometry.seam(wipe: wipe, width: size.width)
                var path = Path()
                path.move(to: CGPoint(x: x, y: 0))
                path.addLine(to: CGPoint(x: x, y: size.height))
                context.stroke(
                    path, with: .color(.white.opacity(210.0 / 255)), lineWidth: 1.5)
                let inset = Self.wipeInset
                // Both captions along the top of the whole picture, because in
                // a wipe there is one picture with a seam through it — the
                // halves have no corners of their own to sit in.
                Self.label(&context, "Before", at: CGPoint(x: inset, y: inset), anchor: .topLeading)
                Self.label(
                    &context, "After", at: CGPoint(x: size.width - inset, y: inset),
                    anchor: .topTrailing)
            case .side:
                let (before, after) = CompareGeometry.halves(in: size, scale: scale)
                let inset = Self.sideInset
                for (rect, text) in [(before, "Before"), (after, "After")] {
                    Self.label(
                        &context, text,
                        at: CGPoint(x: rect.minX + inset, y: rect.minY + inset),
                        anchor: .topLeading)
                }
            }
        }
    }

    /// One caption: eleven point text on a dark plate, `draw_compare`'s
    /// `label`.
    ///
    /// The plate is what makes a caption legible over a photograph at all —
    /// white text alone disappears into a highlight, and a comparison is most
    /// often being read against a bright frame.
    static func label(
        _ context: inout GraphicsContext, _ text: String, at: CGPoint, anchor: UnitPoint
    ) {
        var resolved = context.resolve(Text(text).font(.system(size: 11)))
        resolved.shading = .color(.white.opacity(230.0 / 255))
        let size = resolved.measure(in: CGSize(width: 400, height: 100))
        let origin = CGPoint(
            x: at.x - anchor.x * size.width, y: at.y - anchor.y * size.height)
        let plate = CGRect(origin: origin, size: size).insetBy(dx: -3, dy: -3)
        context.fill(
            Path(roundedRect: plate, cornerRadius: 2),
            with: .color(.black.opacity(150.0 / 255)))
        context.draw(resolved, at: at, anchor: anchor)
    }
}

// -----------------------------------------------------------------------------
// The control
// -----------------------------------------------------------------------------

/// One button for the whole comparison, cycling off → wipe → side → off.
///
/// A `Toggle` because it has to look like the Scopes chip it sits beside and
/// read as chosen the same way — `SELECT` while a comparison is on, which is
/// "this is chosen" rather than the accent's "this is doing something". The
/// value the toggle offers is thrown away and the store cycles instead: three
/// modes are not a switch, and a button that could only turn a comparison on
/// would be the control `Compare.next` exists to avoid.
public struct CompareButton: View {
    let store: SessionStore

    public init(store: SessionStore) {
        self.store = store
    }

    public var body: some View {
        Toggle(
            store.compare.label,
            isOn: Binding(get: { store.compare.on }, set: { _ in store.cycleCompare() })
        )
        .toggleStyle(KromaToggleButtonStyle())
        .help("Off, a wipe, then side by side")
    }
}
