import CoreGraphics
import SwiftUI

/// The chromaticity plot, its pins, and the selected pin's own controls.
///
/// A pin is placed where the colour you care about *is*, dragged to where you
/// want it to go, and told how far around itself to reach. That is a different
/// question from the one the lattices answer — a grid asks what happens to
/// every colour, a pin asks what happens to *this* one — which is why this is
/// a sibling of `WarpEditor` rather than a third pair of axes inside it.
///
/// Resolve draws the spectral locus and the photograph's own colour cloud over
/// this plot. Both need measurements that have no C ABI on this side yet, so
/// what is drawn here is the coordinate space and not the picture. A decorative
/// approximation of the locus would be worse than none: it would be a boundary
/// that looks authoritative and is not, and pins are placed *against* that
/// boundary.
///
/// The in-flight pin is held here and the snapshot is not refreshed mid-drag,
/// for the reason `ScalarRow`, `CurveEditor` and `WarpEditor` do the same.
public struct PinsEditor: View {
    let param: Param
    let row: UInt64
    let value: [PinValue]
    let isActive: Bool
    let store: SessionStore

    /// Which pin the five controls are about. Nil is a real state, not a
    /// missing one: with no pin selected the controls have nothing to act on.
    @State private var chosen: Int?
    /// What the press landed on, decided once when the gesture starts.
    ///
    /// Picking the nearest pin every frame instead would hand the drag to a
    /// neighbour the moment it passed under the pointer — and worse, once the
    /// held pin has been dragged away, the press point has nothing under it at
    /// all, so re-deciding would drop the drag mid-gesture.
    @State private var grab: Grab?
    @State private var live: [PinValue]?

    /// What a press landed on. `empty` is a decision, not the absence of one,
    /// which is why it is a case rather than a nil index.
    private enum Grab: Equatable {
        case pin(Int)
        case empty
    }

    /// CIE D65, where neutral sits. The one landmark this plot can draw
    /// honestly, and without it there is nothing to place a pin *against*.
    public static let whitePoint = CGPoint(x: 0.3127, y: 0.3290)

    public init(
        param: Param, row: UInt64, value: [PinValue], isActive: Bool, store: SessionStore
    ) {
        self.param = param
        self.row = row
        self.value = value
        self.isActive = isActive
        self.store = store
    }

    private var pins: [PinValue] { live ?? value }

    /// The selected pin's index, or nil — including when the selection has
    /// fallen off the end, which an undo can do behind its back. Every use of
    /// the selection goes through here rather than through `chosen`, so a
    /// stale index cannot reach the engine as a refusal in the status bar.
    private var selectedIndex: Int? {
        guard let i = chosen, pins.indices.contains(i) else { return nil }
        return i
    }

    private var selected: PinValue? {
        selectedIndex.map { pins[$0] }
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            plot
            buttons
            controls
        }
        .opacity(isActive ? 1 : 0.4)
        .disabled(!isActive)
    }

    // ---- the plot --------------------------------------------------------

    private var plot: some View {
        GeometryReader { geo in
            canvas(in: geo.size)
        }
        .aspectRatio(1, contentMode: .fit)
        .frame(maxWidth: 320)
    }

    /// Split out of `plot` so the type checker sees one small expression at a
    /// time rather than a ZStack, a gesture and four geometry lets at once.
    private func canvas(in size: CGSize) -> some View {
        let side = min(size.width, size.height)
        let rect = CGRect(
            x: (size.width - side) / 2,
            y: (size.height - side) / 2,
            width: side, height: side
        )
        let g = PinGeometry(pins: pins, rect: rect)
        return ZStack {
            background(rect)
            grid(rect)
            neutral(g)
            marks(g)
        }
        .contentShape(Rectangle())
        .gesture(drag(g))
    }

    private func background(_ rect: CGRect) -> some View {
        RoundedRectangle(cornerRadius: 3)
            .fill(.black.opacity(0.28))
            .overlay(
                RoundedRectangle(cornerRadius: 3)
                    .strokeBorder(.quaternary, lineWidth: 1)
            )
            .frame(width: rect.width, height: rect.height)
            .position(x: rect.midX, y: rect.midY)
    }

    /// A faint grid, so a position can be read rather than only seen.
    private func grid(_ rect: CGRect) -> some View {
        Path { p in
            for i in 1..<4 {
                let t = CGFloat(i) / 4
                let x = rect.minX + t * rect.width
                let y = rect.minY + t * rect.height
                p.move(to: CGPoint(x: x, y: rect.minY))
                p.addLine(to: CGPoint(x: x, y: rect.maxY))
                p.move(to: CGPoint(x: rect.minX, y: y))
                p.addLine(to: CGPoint(x: rect.maxX, y: y))
            }
        }
        .stroke(.white.opacity(0.07), lineWidth: 1)
    }

    /// The white point, as a small cross. Drawn under the pins, because it is
    /// a landmark and not one of them.
    private func neutral(_ g: PinGeometry) -> some View {
        let c = g.screen(of: Self.whitePoint)
        return Path { p in
            p.move(to: CGPoint(x: c.x - 4, y: c.y))
            p.addLine(to: CGPoint(x: c.x + 4, y: c.y))
            p.move(to: CGPoint(x: c.x, y: c.y - 4))
            p.addLine(to: CGPoint(x: c.x, y: c.y + 4))
        }
        .stroke(.white.opacity(0.35), lineWidth: 1)
    }

    private func marks(_ g: PinGeometry) -> some View {
        ForEach(Array(pins.enumerated()), id: \.offset) { i, pin in
            mark(pin, at: i, in: g)
        }
    }

    /// One pin: how far it reaches, where the colour was, where it is going.
    ///
    /// The origin is a ring and the handle is solid — one says where the colour
    /// was, the other where it is being taken. They are drawn differently
    /// because only one of them can be grabbed.
    @ViewBuilder
    private func mark(_ pin: PinValue, at index: Int, in g: PinGeometry) -> some View {
        let from = g.screen(of: pin.at)
        let to = g.screen(of: pin.to)
        let on = chosen == index
        let tint: Color = on ? .accentColor : .white.opacity(0.82)
        let reach = max(g.reach(chromaRange: pin.chromaRange), 2)

        // How far the pin reaches, which is the control people forget is there
        // until they can see it.
        Circle()
            .strokeBorder(tint.opacity(0.45), lineWidth: 1)
            .frame(width: reach * 2, height: reach * 2)
            .position(from)

        if hypot(to.x - from.x, to.y - from.y) > 0.5 {
            Path { p in
                p.move(to: from)
                p.addLine(to: to)
            }
            .stroke(tint, lineWidth: 1.2)
        }

        Circle()
            .strokeBorder(tint, lineWidth: 1.2)
            .frame(width: 6, height: 6)
            .position(from)

        Circle()
            .fill(tint)
            .frame(width: on ? 10 : 8, height: on ? 10 : 8)
            .position(to)
    }

    // ---- dragging --------------------------------------------------------

    private func drag(_ g: PinGeometry) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { gesture in
                if grab == nil {
                    // A press on empty plot selects nothing; on a pin, selects
                    // it. Decided from where the press began, once.
                    if let hit = g.grabbed(at: gesture.startLocation) {
                        grab = .pin(hit)
                        chosen = hit
                        store.beginInteraction(param.name)
                        live = value
                    } else {
                        grab = .empty
                        chosen = nil
                    }
                }
                guard case let .pin(i) = grab else { return }
                let to = g.chromaticity(gesture.location)
                live = pins.replacing(at: i, to: to)
                store.movePin(row: row, key: param.key, index: i, to: to)
            }
            .onEnded { _ in
                if case .pin = grab { store.endInteraction() }
                grab = nil
                live = nil
            }
    }

    // ---- placing and taking away -----------------------------------------

    @ViewBuilder
    private var buttons: some View {
        HStack(spacing: 6) {
            Button("Add pin") { add() }
                .disabled(pins.count >= PinValue.maxPins)
                .help("Places a pin at the white point")

            Button("Delete") { remove() }
                .disabled(selectedIndex == nil)

            Text(tally)
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
    }

    private var tally: String {
        switch pins.count {
        case 0: "add a pin, then drag it"
        case 1: "1 pin"
        case let n: "\(n) pins"
        }
    }

    /// Select what was just made — but only if it was made. `addPin` answers
    /// nil when the engine refused, and selecting on the strength of that would
    /// select a pin that is not there.
    private func add() {
        if let i = store.addPin(row: row, key: param.key, at: Self.whitePoint) {
            chosen = i
        }
    }

    private func remove() {
        guard let i = selectedIndex else { return }
        store.removePin(row: row, key: param.key, index: i)
        chosen = nil
    }

    // ---- the selected pin's own controls ---------------------------------

    /// Dimmed with nothing selected, which is how Resolve draws them and the
    /// only honest thing to do — they have nothing to act on. Shown at a fresh
    /// pin's values rather than blank, so the rows read as controls waiting for
    /// a pin rather than as controls that failed to load.
    @ViewBuilder
    private var controls: some View {
        let pin = selected ?? PinValue.placed(at: Self.whitePoint)
        Text("Pin")
            .font(.caption.weight(.semibold))
            .foregroundStyle(.secondary)

        ForEach(Control.allCases, id: \.self) { control in
            ScalarRow(
                name: control.name,
                unit: control.unit,
                value: control.value(of: pin),
                bounds: control.bounds,
                isActive: selected != nil,
                onChange: { apply(control, of: pin, to: $0) },
                onBegin: { store.beginInteraction(control.name) },
                onEnd: { store.endInteraction() }
            )
        }
    }

    /// One control's change, sent as the whole shape.
    ///
    /// Five parameters written separately would be five calls racing into the
    /// history, and a drag on one of them would be an undo step per frame per
    /// parameter. The engine takes the shape whole for exactly that reason.
    private func apply(_ control: Control, of pin: PinValue, to v: Float) {
        guard let i = selectedIndex else { return }
        let d = Double(v)
        store.setPinShape(
            row: row, key: param.key, index: i,
            chromaRange: control == .chromaRange ? d : pin.chromaRange,
            tonalLow: control == .tonalLow ? d : pin.tonalLow,
            tonalHigh: control == .tonalHigh ? d : pin.tonalHigh,
            tonalPivot: control == .tonalPivot ? d : pin.tonalPivot,
            exposure: control == .exposure ? d : pin.exposure
        )
    }

    /// The five floats inside a pin, as rows.
    ///
    /// A table rather than five hand-written rows: the ranges are the Windows
    /// shell's and the resting values are `Pin::placed`'s, and both are the
    /// kind of number that goes wrong quietly when it is copied five times.
    enum Control: CaseIterable {
        case chromaRange, tonalLow, tonalHigh, tonalPivot, exposure

        var name: String {
            switch self {
            case .chromaRange: "Chroma Range"
            case .tonalLow: "Tonal Range Low"
            case .tonalHigh: "Tonal Range High"
            case .tonalPivot: "Tonal Range Pivot"
            case .exposure: "Exposure"
            }
        }

        /// Stops, for the one of the five that is in them.
        var unit: String {
            self == .exposure ? "EV" : ""
        }

        var bounds: Bounds {
            switch self {
            case .chromaRange: Bounds(min: 0, max: 0.5, default: 0.04, neutral: 0.04)
            case .tonalLow: Bounds(min: 0, max: 1, default: 1, neutral: 1)
            case .tonalHigh: Bounds(min: 0, max: 1, default: 1, neutral: 1)
            case .tonalPivot: Bounds(min: 0, max: 1, default: 0.5, neutral: 0.5)
            case .exposure: Bounds(min: -2, max: 2, default: 0, neutral: 0)
            }
        }

        func value(of pin: PinValue) -> Float {
            switch self {
            case .chromaRange: Float(pin.chromaRange)
            case .tonalLow: Float(pin.tonalLow)
            case .tonalHigh: Float(pin.tonalHigh)
            case .tonalPivot: Float(pin.tonalPivot)
            case .exposure: Float(pin.exposure)
            }
        }
    }
}

extension Array where Element == PinValue {
    /// One pin dragged, the rest left alone. Only `to` moves — `at` is where
    /// the colour is, and moving it would be a different edit entirely.
    func replacing(at index: Int, to: CGPoint) -> [PinValue] {
        guard indices.contains(index) else { return self }
        var next = self
        let p = next[index]
        next[index] = PinValue(
            at: p.at, to: to, chromaRange: p.chromaRange,
            tonalLow: p.tonalLow, tonalHigh: p.tonalHigh,
            tonalPivot: p.tonalPivot, exposure: p.exposure
        )
        return next
    }
}
