import CoreGraphics
import SwiftUI

// -----------------------------------------------------------------------------
// What a drag has hold of
// -----------------------------------------------------------------------------

/// Which part of the rectangle a drag has hold of — `crop.rs`'s `Grip`.
public enum CropGrip: Equatable, Sendable {
    /// The interior: the crop moves and keeps its size.
    case move
    /// One or two of the four sides. Two of them is a corner.
    case edge(left: Bool, right: Bool, top: Bool, bottom: Bool)

    /// How close, in points, the pointer has to be to grab an edge or a corner.
    public static let grab: CGFloat = 14

    /// The smallest crop the tool will let you make, as a fraction of the
    /// frame. Small enough to be no practical limit, large enough that the
    /// handles never end up on top of each other.
    public static let minimumSize: CGFloat = 0.02

    /// Work out what the pointer is over, in the same coordinates as `rect`.
    ///
    /// Corners win over edges because they are the smaller target and the one
    /// the user had to aim for; the interior wins only when nothing else is
    /// close.
    public static func at(_ point: CGPoint, in rect: CGRect, grab: CGFloat = grab) -> CropGrip? {
        let left = abs(point.x - rect.minX) <= grab
        let right = abs(point.x - rect.maxX) <= grab
        let top = abs(point.y - rect.minY) <= grab
        let bottom = abs(point.y - rect.maxY) <= grab

        let insideX = point.x >= rect.minX - grab && point.x <= rect.maxX + grab
        let insideY = point.y >= rect.minY - grab && point.y <= rect.maxY + grab
        guard insideX, insideY else { return nil }

        if left || right || top || bottom {
            return .edge(
                left: left && insideY, right: right && insideY,
                top: top && insideX, bottom: bottom && insideX)
        }
        return rect.contains(point) ? .move : nil
    }

    /// Apply a drag to the rectangle — `crop.rs`'s `dragged`.
    ///
    /// The delta is measured from where the drag *started*, against the
    /// rectangle as it was then, rather than accumulated frame by frame. The
    /// engine corrects every proposal, so accumulating would fold each
    /// correction into the next frame's starting point: push a corner past the
    /// edge, and dragging back would no longer retrace the same rectangle.
    public static func dragged(
        _ rect: CGRect, grip: CropGrip, by delta: CGSize, ratio: Double? = nil
    ) -> CGRect {
        var minX = rect.minX
        var maxX = rect.maxX
        var minY = rect.minY
        var maxY = rect.maxY

        switch grip {
        case .move:
            minX += delta.width
            maxX += delta.width
            minY += delta.height
            maxY += delta.height
        case let .edge(left, right, top, bottom):
            if left { minX = min(minX + delta.width, maxX - minimumSize) }
            if right { maxX = max(maxX + delta.width, minX + minimumSize) }
            if top { minY = min(minY + delta.height, maxY - minimumSize) }
            if bottom { maxY = max(maxY + delta.height, minY + minimumSize) }
        }

        // Hold the locked ratio by moving whichever edges the drag was not
        // already moving, so the corner under the pointer stays under it.
        if let ratio, ratio > 0, case let .edge(left, right, top, bottom) = grip {
            let wantHeight = (maxX - minX) / CGFloat(ratio)
            if abs(wantHeight - (maxY - minY)) > 1e-9 {
                if top && !bottom {
                    minY = maxY - wantHeight
                } else if bottom && !top {
                    maxY = minY + wantHeight
                } else if left || right {
                    let middle = (minY + maxY) / 2
                    minY = middle - wantHeight / 2
                    maxY = middle + wantHeight / 2
                }
            }
        }
        return CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
    }
}

// -----------------------------------------------------------------------------
// The overlay
// -----------------------------------------------------------------------------

/// The crop rectangle over the photograph, and the drag that moves it.
///
/// The tool's frame is the **enclosing** one — the whole source, straightened —
/// rather than the cropped result, which is what makes the rectangle draggable:
/// the user can see what is outside the crop and pull it back in. The engine
/// draws that frame while `SessionStore.setCropping` is on, and answers where
/// the crop sits inside it; nothing on this side works out either.
///
/// **Everything drawn here is the value the engine gave back**, never the one
/// that was proposed. Each frame of a drag works out a rectangle, hands it to
/// `SessionStore.setCropRect` and reads `store.cropRect` — which mid-drag is
/// the engine's corrected answer rather than the snapshot's copy of it, because
/// the snapshot is deliberately behind until the gesture ends.
///
/// The in-flight rectangle is not held here for the same reason: it is not this
/// side's to decide. `CurveEditor` and `WarpEditor` keep a local value between
/// frames because the engine has nothing to say about theirs; this one does.
///
/// **Nothing here takes the pointer.** The drag that moves the rectangle lives
/// in `MetalViewerView` with the zoom and the pan — see ``ViewerDrag``, which
/// records what a SwiftUI layer in front of that view does to the events it was
/// meant to get. It used to be here, and it was taking the wheel and the
/// double-click along with the drag it wanted.
public struct CropOverlay: View {
    let store: SessionStore

    public init(store: SessionStore) {
        self.store = store
    }

    /// The ratio the rectangle has to hold *on screen*, if it holds one.
    ///
    /// The lock is a property of the picture and is measured in pixels, while
    /// the rectangle here is a fraction of a frame whose two sides are
    /// different numbers of pixels — so the lock has to be carried into the
    /// frame's own proportions before a drag can honour it. It does not have to
    /// be carried by hand: the engine has already applied the lock to the crop
    /// it handed back, so the shape of that rectangle *is* the shape to keep.
    /// A quarter-turn and a source that is not square are both already in it.
    ///
    /// Only a hint. `apply_aspect` in the engine is what actually decides, and
    /// the corrected value is what is drawn; getting this wrong would let the
    /// corner drift out from under the pointer, not produce a crop the document
    /// does not hold.
    static func screenRatio(of aspect: AspectLock, showing rect: CGRect) -> Double? {
        guard aspect != .free, rect.width > 0, rect.height > 0 else { return nil }
        return Double(rect.width / rect.height)
    }

    public var body: some View {
        GeometryReader { geo in
            let target = CGRect(origin: .zero, size: geo.size)
            let rect = Self.place(store.cropRect, in: target, showing: store.view.region)
            CropOverlayCanvas(rect: rect, showsThirds: store.croppingByHand)
        }
        .allowsHitTesting(false)
        .accessibilityLabel("Crop")
    }

    /// Where a rectangle given in the frame's uv lands on screen.
    ///
    /// `target` is the on-screen rectangle and `visible` is the part of the
    /// frame it is showing. The two are not the same thing at any zoom other
    /// than fit, and assuming they were is what pinned the Windows shell's
    /// viewer to fit whenever this tool was open.
    static func place(_ uv: CGRect, in target: CGRect, showing visible: CGRect) -> CGRect {
        let span = CGSize(
            width: max(visible.width, 1e-6), height: max(visible.height, 1e-6))
        let at = { (u: CGFloat, v: CGFloat) in
            CGPoint(
                x: target.minX + (u - visible.minX) / span.width * target.width,
                y: target.minY + (v - visible.minY) / span.height * target.height)
        }
        let a = at(uv.minX, uv.minY)
        let b = at(uv.maxX, uv.maxY)
        return CGRect(x: a.x, y: a.y, width: b.x - a.x, height: b.y - a.y)
    }
}

/// Everything the overlay paints, given the rectangle to paint it around.
///
/// Split from `CropOverlay` so it can be rendered without an engine: what a
/// crop overlay looks like is exactly the kind of thing that stays broken for
/// weeks because nothing can fail on it.
///
/// The colours are absolute rather than from the palette, and deliberately: a
/// dimmed surround and a white rectangle over a photograph are a picture of
/// what is being cut away, not interface furniture. The Windows shell paints
/// the same four values.
struct CropOverlayCanvas: View {
    /// The crop, in the view's own points.
    let rect: CGRect
    /// The thirds grid, which is drawn while a drag is in flight.
    let showsThirds: Bool

    /// How far a corner bracket reaches along each edge, at most.
    static let arm: CGFloat = 18

    var body: some View {
        Canvas { context, size in
            let target = CGRect(origin: .zero, size: size)
            dim(&context, target)
            if showsThirds { thirds(&context) }
            context.stroke(
                Path(rect.insetBy(dx: 0.75, dy: 0.75)),
                with: .color(.white.opacity(0.86)), lineWidth: 1.5)
            brackets(&context)
        }
    }

    /// Everything outside the crop, dimmed.
    ///
    /// Four bands rather than a stencil, which keeps it to four fills and no
    /// allocation — and, unlike an even-odd path, cannot leave the *inside*
    /// dimmed on a rectangle that has been dragged inside out.
    private func dim(_ context: inout GraphicsContext, _ target: CGRect) {
        let shade = Color.black.opacity(0.59)
        let bands = [
            CGRect(x: target.minX, y: target.minY, width: target.width, height: rect.minY - target.minY),
            CGRect(x: target.minX, y: rect.maxY, width: target.width, height: target.maxY - rect.maxY),
            CGRect(x: target.minX, y: rect.minY, width: rect.minX - target.minX, height: rect.height),
            CGRect(x: rect.maxX, y: rect.minY, width: target.maxX - rect.maxX, height: rect.height),
        ]
        for band in bands where band.width > 0 && band.height > 0 {
            context.fill(Path(band), with: .color(shade))
        }
    }

    /// Thirds. The one grid worth drawing by default: it is the composition
    /// most people are checking against when they reach for the crop tool.
    private func thirds(_ context: inout GraphicsContext) {
        var path = Path()
        for i in 1..<3 {
            let t = CGFloat(i) / 3
            let x = rect.minX + t * rect.width
            let y = rect.minY + t * rect.height
            path.move(to: CGPoint(x: x, y: rect.minY))
            path.addLine(to: CGPoint(x: x, y: rect.maxY))
            path.move(to: CGPoint(x: rect.minX, y: y))
            path.addLine(to: CGPoint(x: rect.maxX, y: y))
        }
        context.stroke(path, with: .color(.white.opacity(0.27)), lineWidth: 1)
    }

    /// The eight grips: a bracket at each corner and a bar across the middle of
    /// each edge.
    ///
    /// Brackets rather than square handles, and drawn *inside* the crop, so
    /// they never hide the edge they are attached to.
    private func brackets(_ context: inout GraphicsContext) {
        let arm = min(Self.arm, min(rect.width, rect.height) * 0.3)
        guard arm > 0 else { return }
        var path = Path()
        let corners: [(CGPoint, CGFloat, CGFloat)] = [
            (CGPoint(x: rect.minX, y: rect.minY), 1, 1),
            (CGPoint(x: rect.maxX, y: rect.minY), -1, 1),
            (CGPoint(x: rect.minX, y: rect.maxY), 1, -1),
            (CGPoint(x: rect.maxX, y: rect.maxY), -1, -1),
        ]
        for (corner, dx, dy) in corners {
            // Pulled half a stroke inside, so the bracket sits on the crop
            // rather than straddling its edge.
            let at = CGPoint(x: corner.x + 1.5 * dx, y: corner.y + 1.5 * dy)
            path.move(to: CGPoint(x: at.x + arm * dx, y: at.y))
            path.addLine(to: at)
            path.addLine(to: CGPoint(x: at.x, y: at.y + arm * dy))
        }
        // And the four edge grips, each a short bar across the middle of its
        // side — the other half of the eight the tool offers.
        for (from, to) in [
            (CGPoint(x: rect.midX - arm / 2, y: rect.minY + 1.5),
                CGPoint(x: rect.midX + arm / 2, y: rect.minY + 1.5)),
            (CGPoint(x: rect.midX - arm / 2, y: rect.maxY - 1.5),
                CGPoint(x: rect.midX + arm / 2, y: rect.maxY - 1.5)),
            (CGPoint(x: rect.minX + 1.5, y: rect.midY - arm / 2),
                CGPoint(x: rect.minX + 1.5, y: rect.midY + arm / 2)),
            (CGPoint(x: rect.maxX - 1.5, y: rect.midY - arm / 2),
                CGPoint(x: rect.maxX - 1.5, y: rect.midY + arm / 2)),
        ] {
            path.move(to: from)
            path.addLine(to: to)
        }
        context.stroke(path, with: .color(.white), lineWidth: 3)
    }
}
