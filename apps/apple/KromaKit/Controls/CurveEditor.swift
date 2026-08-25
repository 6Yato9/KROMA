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

/// The photograph's own measurements, behind the curve that acts on them.
///
/// A curve editor with nothing behind it is a diagram of a function: you can
/// see the shape you have drawn and not the thing you drew it for. Every peak
/// here is a tone — or a hue, or a saturation — the picture actually has, and
/// moving the curve *there* rather than a third of the way along is the whole
/// difference between grading and guessing.
///
/// *Which* measurement belongs behind a given curve is `CurveBackdrop.behind`,
/// checked against the engine's own table rather than decided here. This draws
/// whichever one it is handed.
struct CurveBackdropView: View {
    let backdrop: CurveBackdrop
    /// The last measurement, or nil when there is none.
    ///
    /// Nil draws nothing, and in particular does not ask for one. The editor
    /// is not what decides when to measure — a measurement is a full extra
    /// render plus a 1.2 MB readback, and a view that started one from its own
    /// body would do it on every layout pass.
    let scopes: Scopes?
    /// The same margin the curve is drawn in, so the two share an origin.
    let inset: CGFloat

    /// How much of the plot a full-scale peak fills.
    ///
    /// Short of the top edge, so a peak that reaches full scale does not touch
    /// the ceiling and read as clipped when it is not.
    private static let ceiling = 0.92

    var body: some View {
        Canvas { context, size in
            guard let scopes else { return }
            let canvas = CurveCanvas(size: size, inset: inset)
            switch backdrop {
            case .tones:
                tones(&context, canvas, scopes.logHistogram)
            case .luma:
                // The luma trace is the tone drawing with one plane, read
                // through the same SDR window — which is exactly what the
                // shader's `lum_in` indexes.
                let levels = scopes.logHistogram
                single(
                    &context, canvas,
                    CurveBackdrop.trace(levels.plane(.luma), peak: Double(levels.fullScale)),
                    windowed: true)
            case .hue:
                let spread = scopes.colour
                single(
                    &context, canvas,
                    CurveBackdrop.trace(spread.hue, peak: Double(spread.fullScale)),
                    windowed: false)
            case .saturation:
                let spread = scopes.colour
                single(
                    &context, canvas,
                    CurveBackdrop.trace(spread.saturation, peak: Double(spread.fullScale)),
                    windowed: false)
            case .nothing:
                // Nothing is known to belong behind this curve, so nothing is
                // drawn. A plausible count of the wrong quantity would be the
                // worse of the two, and it is not an error either way.
                break
            }
        }
        .allowsHitTesting(false)
    }

    /// The three channels overlaid, each a filled area with a line along its
    /// top.
    ///
    /// **Added, not blended.** Three translucent layers painted in order leave
    /// the last one on top, so a neutral picture — one where all three
    /// channels agree — comes out whichever colour is drawn last; the other
    /// shell's came out blue, because blue is drawn last. Adding makes
    /// agreement read as grey and disagreement as the colour that is in
    /// excess, which is the entire reason for overlaying them.
    private func tones(
        _ context: inout GraphicsContext, _ canvas: CurveCanvas, _ levels: Scopes.Levels
    ) {
        context.blendMode = .plusLighter
        for channel in [Scopes.Channel.red, .green, .blue] {
            let heights = CurveBackdrop.trace(
                levels.plane(channel), peak: Double(levels.fullScale))
            let ink = ScopeImage.tint(channel)
            let colour = Color(red: ink.red, green: ink.green, blue: ink.blue)
            context.fill(
                area(heights, canvas, windowed: true),
                with: .color(colour.opacity(0.22)))
            // Added like the fill under it: where the three agree the outlines
            // land on one another and sum towards white, which is what "this
            // picture is neutral here" should look like; where they part, each
            // keeps its own colour.
            context.stroke(
                top(heights, canvas, windowed: true),
                with: .color(colour.opacity(0.55)), lineWidth: 1.2)
        }
    }

    /// One plane, drawn neutral.
    ///
    /// Grey rather than one of the channel colours: a curve indexed by
    /// luminance, hue or saturation reads a single number off the picture, and
    /// three coloured traces would be saying something about channels its
    /// x-axis never asks about. One layer, so there is nothing to add it to.
    private func single(
        _ context: inout GraphicsContext, _ canvas: CurveCanvas,
        _ heights: [Double], windowed: Bool
    ) {
        context.fill(
            area(heights, canvas, windowed: windowed),
            with: .color(Color(white: 0.9).opacity(0.19)))
        context.stroke(
            top(heights, canvas, windowed: windowed),
            with: .color(.white.opacity(0.82)), lineWidth: 1.2)
    }

    /// Which bin a fraction across the plot reads from.
    ///
    /// A tone plot's edges are diffuse black and diffuse white, so its bins are
    /// read through that window; a hue runs once round the circle and a
    /// saturation from nothing to full, so both fill their plot edge to edge.
    /// Laid out edge to edge instead, every tone would sit about a seventh of
    /// the plot to the left of where the curve acts on it.
    private func bin(_ fraction: Double, windowed: Bool) -> Int {
        windowed
            ? CurveBackdrop.bin(atPlotFraction: fraction)
            : CurveBackdrop.spreadBin(atPlotFraction: fraction)
    }

    /// The outline of one trace across the plot.
    private func top(_ heights: [Double], _ canvas: CurveCanvas, windowed: Bool) -> Path {
        Path { p in
            guard heights.count > 1 else { return }
            let steps = CurveBackdrop.binCount - 1
            for i in 0...steps {
                let t = Double(i) / Double(steps)
                let h = heights[min(bin(t, windowed: windowed), heights.count - 1)]
                let point = canvas.view(CGPoint(x: t, y: h * Self.ceiling))
                if i == 0 { p.move(to: point) } else { p.addLine(to: point) }
            }
        }
    }

    /// Its filled body, down to the plot's own baseline — the same zero the
    /// curve is drawn against, not the bottom of the view.
    private func area(_ heights: [Double], _ canvas: CurveCanvas, windowed: Bool) -> Path {
        var path = top(heights, canvas, windowed: windowed)
        guard heights.count > 1 else { return path }
        path.addLine(to: canvas.view(CGPoint(x: 1, y: 0)))
        path.addLine(to: canvas.view(CGPoint(x: 0, y: 0)))
        path.closeSubpath()
        return path
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
                backdrop
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

    /// The picture behind the curve: the tones this curve acts on, or the hues
    /// or saturations, or nothing at all.
    ///
    /// Under the grid, the identity line, the curve and its handles, because a
    /// backdrop painted over them is a backdrop that has eaten the control.
    /// Which measurement belongs here is `CurveBackdrop.behind`, asked by key
    /// rather than decided from the effect — Custom Curves edits four of them
    /// and the editor drawing one already knows which.
    private var backdrop: some View {
        CurveBackdropView(
            backdrop: CurveBackdrop.behind(param.key),
            scopes: store.scopes,
            inset: Self.inset
        )
    }

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
