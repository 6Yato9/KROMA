import CoreGraphics
import SwiftUI

/// Maps between a curve's unit square and the view's pixels.
///
/// Separate from the view because the y-flip is a sign convention, and a sign
/// convention that is only checked by looking at it is one that stays wrong
/// until somebody notices their curves are upside down.
public struct CurveCanvas {
    public let size: CGSize
    /// Room around the plot so a handle sitting on 0 or 1 is fully drawn and
    /// fully grabbable — otherwise the two points most often dragged are the
    /// two hardest to hit.
    public let inset: CGFloat

    public init(size: CGSize, inset: CGFloat) {
        self.size = size
        self.inset = inset
    }

    private var plot: CGSize {
        CGSize(
            width: max(size.width - inset * 2, 0),
            height: max(size.height - inset * 2, 0)
        )
    }

    /// Curve coordinates to view coordinates.
    public func view(_ p: CGPoint) -> CGPoint {
        CGPoint(
            x: inset + p.x * plot.width,
            y: inset + (1 - p.y) * plot.height
        )
    }

    /// View coordinates back to curve coordinates.
    ///
    /// Zero-sized on the first layout pass, and a division by zero there puts a
    /// NaN into the path, which blanks the panel rather than misdrawing it.
    public func curve(_ p: CGPoint) -> CGPoint {
        let w = plot.width, h = plot.height
        return CGPoint(
            x: w > 0 ? (p.x - inset) / w : 0,
            y: h > 0 ? 1 - (p.y - inset) / h : 0
        )
    }
}

/// One curve, drawn and editable.
///
/// Drag a point to move it, press empty space to add one, and double-click a
/// point to take it out. The ends may be moved vertically but not horizontally
/// and may not be removed — they anchor the range the engine evaluates over.
///
/// The in-flight curve is held here and the snapshot is not refreshed mid-drag,
/// for the reason `FloatRow` does the same: dragging should not diff the
/// document sixty times a second.
public struct CurveEditor: View {
    let param: Param
    let flat: Bool
    let row: UInt64
    let value: CurveValue
    let isActive: Bool
    let store: SessionStore

    @State private var dragging: CurveGeometry?
    @State private var heldIndex: Int?

    private static let inset: CGFloat = 7
    private static let reach: CGFloat = 0.06

    public init(
        param: Param, flat: Bool, row: UInt64, value: CurveValue,
        isActive: Bool, store: SessionStore
    ) {
        self.param = param
        self.flat = flat
        self.row = row
        self.value = value
        self.isActive = isActive
        self.store = store
    }

    private var curve: CurveGeometry {
        dragging ?? CurveGeometry(points: value.points)
    }

    public var body: some View {
        GeometryReader { geo in
            let canvas = CurveCanvas(size: geo.size, inset: Self.inset)
            ZStack {
                grid(canvas)
                identity(canvas)
                line(canvas)
                handles(canvas)
            }
            .contentShape(Rectangle())
            .gesture(drag(canvas))
            .onTapGesture(count: 2) { location in
                remove(near: canvas.curve(location))
            }
        }
        .frame(height: 168)
        .background(.black.opacity(0.28))
        .overlay(
            RoundedRectangle(cornerRadius: 3)
                .strokeBorder(.quaternary, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: 3))
        .opacity(isActive ? 1 : 0.4)
        .disabled(!isActive)
    }

    // ---- drawing ---------------------------------------------------------

    private func grid(_ canvas: CurveCanvas) -> some View {
        Path { p in
            for i in 1..<4 {
                let t = CGFloat(i) / 4
                p.move(to: canvas.view(CGPoint(x: t, y: 0)))
                p.addLine(to: canvas.view(CGPoint(x: t, y: 1)))
                p.move(to: canvas.view(CGPoint(x: 0, y: t)))
                p.addLine(to: canvas.view(CGPoint(x: 1, y: t)))
            }
        }
        .stroke(.white.opacity(0.07), lineWidth: 1)
    }

    /// Where this curve rests. A tone curve's identity is the diagonal; a
    /// secondary's is a level line down the middle, because it answers a
    /// different question. Drawing it makes "how far have I pushed this"
    /// answerable at a glance.
    private func identity(_ canvas: CurveCanvas) -> some View {
        Path { p in
            if flat {
                p.move(to: canvas.view(CGPoint(x: 0, y: 0.5)))
                p.addLine(to: canvas.view(CGPoint(x: 1, y: 0.5)))
            } else {
                p.move(to: canvas.view(CGPoint(x: 0, y: 0)))
                p.addLine(to: canvas.view(CGPoint(x: 1, y: 1)))
            }
        }
        .stroke(.white.opacity(0.15), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
    }

    private func line(_ canvas: CurveCanvas) -> some View {
        let g = curve
        return Path { p in
            // Sampled at the LUT's own resolution, so what is drawn is what is
            // baked — no smoothing that exists only on screen.
            let steps = 128
            for i in 0...steps {
                let x = Double(i) / Double(steps)
                let point = canvas.view(CGPoint(x: x, y: g.sample(at: x)))
                if i == 0 { p.move(to: point) } else { p.addLine(to: point) }
            }
        }
        .stroke(tint, style: StrokeStyle(lineWidth: 1.6, lineCap: .round))
    }

    private func handles(_ canvas: CurveCanvas) -> some View {
        let g = curve
        return ForEach(Array(g.points.enumerated()), id: \.offset) { index, point in
            Circle()
                .fill(index == heldIndex ? tint : Color.white)
                .frame(width: 6, height: 6)
                .position(canvas.view(point))
        }
    }

    /// The channel's own colour, so a red curve reads as red without a label.
    private var tint: Color {
        switch param.key {
        case "red": .red
        case "green": .green
        case "blue": .blue
        default: .white
        }
    }

    // ---- editing ---------------------------------------------------------

    private func drag(_ canvas: CurveCanvas) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { g in
                let at = canvas.curve(g.location)
                if dragging == nil {
                    store.beginInteraction(param.name)
                    var start = CurveGeometry(points: value.points)
                    // Grab the nearest point, or put one down where the user
                    // pressed. Adding on press rather than on release means the
                    // new point is draggable within the same gesture, which is
                    // how it is done everywhere else and what a hand expects.
                    if let i = start.indexOfPoint(near: at, within: Self.reach) {
                        heldIndex = i
                    } else {
                        start = start.adding(at)
                        heldIndex = start.indexOfPoint(near: at, within: Self.reach)
                    }
                    dragging = start
                }
                guard let i = heldIndex else { return }
                let moved = (dragging ?? CurveGeometry(points: value.points))
                    .moving(at: i, to: at)
                dragging = moved
                // The held point can change index if the sort reorders around
                // it; follow it rather than dragging whatever now sits at `i`.
                heldIndex = moved.indexOfPoint(near: at, within: Self.reach) ?? i
                store.setCurve(row: row, key: param.key, points: moved.points)
            }
            .onEnded { _ in
                store.endInteraction()
                dragging = nil
                heldIndex = nil
            }
    }

    private func remove(near location: CGPoint) {
        let g = CurveGeometry(points: value.points)
        guard let i = g.indexOfPoint(near: location, within: Self.reach),
              g.canRemovePoint(at: i)
        else { return }
        store.setCurve(row: row, key: param.key, points: g.removing(at: i).points)
    }
}

/// The curves of one effect, behind a picker.
///
/// Custom Curves has ten. Stacking ten editors would be a panel nobody can use,
/// so this shows one at a time — the same shape as the Windows shell's row of
/// channel buttons. An effect with a single curve gets no picker.
public struct CurvePanel: View {
    let effect: Effect
    let params: [Param]
    let row: Snapshot.Row
    let store: SessionStore

    @State private var selected: String?

    public init(effect: Effect, params: [Param], row: Snapshot.Row, store: SessionStore) {
        self.effect = effect
        self.params = params
        self.row = row
        self.store = store
    }

    private var current: Param? {
        params.first { $0.key == selected } ?? params.first
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if params.count > 1 {
                Picker("", selection: Binding(
                    get: { current?.key ?? "" },
                    set: { selected = $0 }
                )) {
                    ForEach(params) { p in
                        Text(p.name).tag(p.key)
                    }
                }
                .labelsHidden()
                .pickerStyle(.menu)
                .controlSize(.small)
            }

            if let param = current, case let .curve(flat) = param.kind {
                CurveEditor(
                    param: param,
                    flat: flat,
                    row: row.id,
                    value: row.params[param.key]?.curveValue
                        ?? CurveValue(points: flat
                            ? [CGPoint(x: 0, y: 0.5), CGPoint(x: 1, y: 0.5)]
                            : [CGPoint(x: 0, y: 0), CGPoint(x: 1, y: 1)]),
                    isActive: effect.isActive(param.key, values: row.params),
                    store: store
                )
            }
        }
        .padding(.bottom, 4)
    }
}
