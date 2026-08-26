import CoreGraphics
import SwiftUI

// -----------------------------------------------------------------------------
// Where the crop sits in the frame the viewer is showing
// -----------------------------------------------------------------------------

/// The crop rectangle, expressed in the frame the viewer is drawing.
///
/// The tool's frame is the **enclosing** one — the whole source, straightened —
/// rather than the cropped result. That is what makes the rectangle draggable:
/// the user can see what is outside the crop and pull it back in, and because
/// the enclosing frame carries the same angle, turn and flips as the crop, the
/// rectangle stays axis-aligned on screen at any angle. `apps/windows/src/crop.rs`
/// says the same thing at the top of the file, and this is its counterpart.
///
/// **This is a second copy of arithmetic that lives in `pe_core::geometry`,
/// and that is a deliberate, reluctant exception.** `Geometry::enclosing`,
/// `crop_uv_in` and `set_crop_uv_in` are what the Windows shell calls; the C
/// ABI carries only the seven scalar fields of a `Geometry` (nine in, seven
/// out — see `pe_session_set_geometry`), so there is no call that would answer
/// the question this side has to ask sixty times a second. Everything about
/// what makes a geometry *legal* — `apply_aspect`, `slide_to_fit`,
/// `shrink_to_fit` — stays in Rust, is never reimplemented here, and reaches
/// this file only as the value `Session.setGeometry` hands back.
///
/// The copy is pinned in `CropOverlayTests` against numbers taken from
/// `pe_core`'s own `crop_uv_in`, including a quarter-turn and both flips: the
/// permutation is the part that looks entirely plausible when it is wrong.
public struct CropFrame: Equatable, Sendable {
    /// The crop being edited — the document's geometry.
    public let crop: GeometryValue
    /// The geometry the viewer is showing, which for this tool is
    /// ``CropFrame/enclosing(_:source:)`` of the crop.
    ///
    /// The two must share an angle, a turn and both flips; the enclosing frame
    /// keeps all four, which is the only reason the crop is an axis-aligned
    /// rectangle in the frame's own coordinates.
    public let frame: GeometryValue
    /// The source photograph, in pixels.
    public let source: CGSize

    public init(crop: GeometryValue, frame: GeometryValue, source: CGSize) {
        self.crop = crop
        self.frame = frame
        self.source = source
    }

    /// The crop against the frame the tool shows it in.
    public init(crop: GeometryValue, source: CGSize) {
        self.init(crop: crop, frame: Self.enclosing(crop, source: source), source: source)
    }

    /// The frame the crop tool shows: the whole source, straightened.
    ///
    /// `pe_core::Geometry::enclosing`. Big enough to hold the rotated picture,
    /// so the blank corners are visible and the user can see exactly what
    /// straightening is costing them. Keeps the turns and the flips; drops the
    /// aspect lock, which constrains the crop and not the frame around it.
    public static func enclosing(_ g: GeometryValue, source: CGSize) -> GeometryValue {
        let sw = max(source.width, 1)
        let sh = max(source.height, 1)
        let r = g.angle * .pi / 180
        let (s, c) = (abs(sin(r)), abs(cos(r)))
        return GeometryValue(
            centre: .zero,
            size: CGSize(width: (sw * c + sh * s) / sw, height: (sw * s + sh * c) / sh),
            angle: g.angle, turns: g.turns, flipH: g.flipH, flipV: g.flipV,
            aspect: .free
        )
    }

    /// Pixel size of a geometry's result — `pe_core::Geometry::output_size`.
    ///
    /// Rounded to whole pixels, because that is what the engine stores and a
    /// rectangle drawn from unrounded sizes would sit a fraction of a pixel
    /// away from the crop it is drawing.
    static func outputSize(_ g: GeometryValue, source: CGSize) -> CGSize {
        let w = max((abs(g.size.width) * source.width).rounded(), 1)
        let h = max((abs(g.size.height) * source.height).rounded(), 1)
        return turns(g) % 2 == 1 ? CGSize(width: h, height: w) : CGSize(width: w, height: h)
    }

    /// Quarter-turns, brought into 0…3 the way the engine does.
    static func turns(_ g: GeometryValue) -> Int { ((g.turns % 4) + 4) % 4 }

    /// One quarter-turn of a vector, `pe_core::Geometry::sampling`'s `quarter`.
    static func quarter(_ v: CGPoint, _ turns: Int) -> CGPoint {
        switch ((turns % 4) + 4) % 4 {
        case 0: CGPoint(x: v.x, y: v.y)
        case 1: CGPoint(x: v.y, y: -v.x)
        case 2: CGPoint(x: -v.x, y: -v.y)
        default: CGPoint(x: -v.y, y: v.x)
        }
    }

    /// The flips, which act on the output and are their own inverse.
    static func flipped(_ v: CGPoint, h: Bool, v flipV: Bool) -> CGPoint {
        CGPoint(x: h ? -v.x : v.x, y: flipV ? -v.y : v.y)
    }

    /// Where the crop sits inside the frame, as a rectangle in the frame's own
    /// uv — `pe_core::Geometry::crop_uv_in`, for the case this tool has.
    ///
    /// Both geometries map their output uv into the source through the same
    /// straightening, the same turn and the same flips, so all of that cancels
    /// and what is left is the crop's own pixel size against the frame's, and
    /// the offset between the two centres carried back through the turn and the
    /// flips.
    public var rect: CGRect {
        let f = Self.outputSize(frame, source: source)
        let c = Self.outputSize(crop, source: source)
        let offset = CGPoint(
            x: (crop.centre.x - frame.centre.x) * source.width,
            y: (crop.centre.y - frame.centre.y) * source.height
        )
        let e = Self.flipped(
            Self.quarter(offset, 4 - Self.turns(crop)), h: crop.flipH, v: crop.flipV)
        return CGRect(
            x: 0.5 + (e.x - c.width / 2) / f.width,
            y: 0.5 + (e.y - c.height / 2) / f.height,
            width: c.width / f.width,
            height: c.height / f.height
        )
    }

    /// The geometry that would put the crop at `rect` — the inverse of
    /// ``CropFrame/rect``, and `pe_core::Geometry::set_crop_uv_in`.
    ///
    /// A *proposal*. It is what goes into `SessionStore.setGeometry`, and what
    /// comes back out is what gets drawn: the engine re-shapes it to a locked
    /// aspect, slides it back inside the straightened source and shrinks it if
    /// it still will not fit. Drawing this value would put a rectangle on
    /// screen that the renderer does not produce, and it would jump to the real
    /// one the moment the drag ended.
    public func proposing(_ rect: CGRect) -> GeometryValue {
        let f = Self.outputSize(frame, source: source)
        let (ow, oh) = (rect.width * f.width, rect.height * f.height)
        // Undo the quarter-turn's swap: the document stores the crop before it
        // is turned, so an odd turn means the on-screen width is the stored
        // height.
        let size =
            Self.turns(crop) % 2 == 1
            ? CGSize(width: oh / max(source.width, 1), height: ow / max(source.height, 1))
            : CGSize(width: ow / max(source.width, 1), height: oh / max(source.height, 1))
        let e = CGPoint(x: (rect.midX - 0.5) * f.width, y: (rect.midY - 0.5) * f.height)
        let offset = Self.quarter(
            Self.flipped(e, h: crop.flipH, v: crop.flipV), Self.turns(crop))
        return GeometryValue(
            centre: CGPoint(
                x: frame.centre.x + offset.x / max(source.width, 1),
                y: frame.centre.y + offset.y / max(source.height, 1)
            ),
            size: size,
            angle: crop.angle, turns: crop.turns,
            flipH: crop.flipH, flipV: crop.flipV,
            aspect: crop.aspect
        )
    }

    /// The ratio the rectangle holds *on screen*, if it holds one.
    ///
    /// The lock is a property of the picture and is measured in pixels, while
    /// the rectangle here is a fraction of a frame whose two sides are
    /// different numbers of pixels — so the lock has to be carried into the
    /// frame's own proportions before a drag can honour it. A quarter-turn
    /// inverts it, because a 16:9 crop turned on its side is 9:16 on screen.
    ///
    /// Only a hint: `apply_aspect` in the engine is what actually decides, and
    /// the corrected value is what is drawn. Getting this slightly wrong would
    /// let the corner drift out from under the pointer, not produce a crop the
    /// document does not hold.
    ///
    /// Which is why `Original` answers nil rather than the source's own
    /// proportions. `AspectLock.widthOverHeight` leaves that arm to the engine
    /// on purpose, and a lock with no hint still holds — the corner simply does
    /// not track the pointer quite as closely on the way there.
    public var screenRatio: Double? {
        guard let ratio = crop.aspect.widthOverHeight, ratio > 0 else { return nil }
        let f = Self.outputSize(frame, source: source)
        guard f.width > 0, f.height > 0 else { return nil }
        let inPixels = Self.turns(crop) % 2 == 1 ? 1 / ratio : ratio
        return inPixels * Double(f.height / f.width)
    }
}

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
/// **Everything drawn here is the value the engine gave back**, never the one
/// that was proposed. Each frame of a drag works out a rectangle, turns it into
/// a geometry, hands it to `SessionStore.setGeometry` and reads
/// `store.geometry` — which mid-drag is the engine's corrected answer rather
/// than the snapshot's copy of it, because the snapshot is deliberately behind
/// until the gesture ends.
///
/// The in-flight rectangle is not held here for the same reason: it is not this
/// side's to decide. `CurveEditor` and `WarpEditor` keep a local value between
/// frames because the engine has nothing to say about theirs; this one does.
public struct CropOverlay: View {
    let store: SessionStore

    /// The grip a drag caught, decided once when it started. Re-deciding every
    /// frame would hand the drag to whichever handle happened to pass under the
    /// pointer.
    @State private var held: CropGrip?
    /// The rectangle as it was when the drag started, in the frame's uv, and
    /// the model that produced it. Both are fixed for the length of a gesture:
    /// the deltas are measured from here.
    @State private var start: (model: CropFrame, rect: CGRect)?

    public init(store: SessionStore) {
        self.store = store
    }

    /// The crop, against the frame the tool shows it in.
    private var model: CropFrame {
        CropFrame(
            crop: store.geometry,
            source: CGSize(
                width: CGFloat(max(store.snapshot.width, 1)),
                height: CGFloat(max(store.snapshot.height, 1))))
    }

    public var body: some View {
        GeometryReader { geo in
            let target = CGRect(origin: .zero, size: geo.size)
            let rect = Self.place(model.rect, in: target, showing: store.view.region)
            CropOverlayCanvas(rect: rect, showsThirds: held != nil)
                // The whole viewer, not just the rectangle: a drag that starts
                // a few points outside a corner is a drag on that corner, and
                // `CropGrip.at` is what decides — it answers nil for a press
                // that has hold of nothing, and the gesture then does nothing.
                .contentShape(Rectangle())
                .gesture(drag(target: target))
        }
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

    private func drag(target: CGRect) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { gesture in
                let visible = store.view.region
                if held == nil {
                    let current = model
                    let onScreen = Self.place(current.rect, in: target, showing: visible)
                    guard let grip = CropGrip.at(gesture.startLocation, in: onScreen) else {
                        return
                    }
                    store.beginInteraction("Crop")
                    held = grip
                    start = (current, current.rect)
                }
                guard let grip = held, let start else { return }
                // Screen points into the frame's uv. Zoomed in, a point on
                // screen is a smaller step across the frame, which is what
                // `visible` carries.
                let delta = CGSize(
                    width: gesture.translation.width / max(target.width, 1e-4) * visible.width,
                    height: gesture.translation.height / max(target.height, 1e-4) * visible.height
                )
                let next = CropGrip.dragged(
                    start.rect, grip: grip, by: delta, ratio: start.model.screenRatio)
                store.setGeometry(start.model.proposing(next))
            }
            .onEnded { _ in
                if held != nil { store.endInteraction() }
                held = nil
                start = nil
            }
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
