import SwiftUI

/// Resolve's inspector row: a right-aligned label, a thin track with a round
/// handle, a boxed number, and a reset arrow.
///
/// The column widths are Resolve's own, read off the Windows shell's
/// `resolve.rs`. Getting the row right once is most of what makes a panel look
/// like Resolve; doing it by hand at each call site is how the columns end up
/// not lining up, which is immediately obvious in a panel of thirty controls.
enum RowMetrics {
    static let label: CGFloat = 112
    static let value: CGFloat = 58
    static let reset: CGFloat = 18
    static let gap: CGFloat = 6
    static let height: CGFloat = 22
}

/// The look of an inspector row — label, track, readout, reset arrow — without
/// any opinion about where the number lives.
///
/// `FloatRow` drives a registry parameter through the store. A pin's five
/// controls are floats *inside* a parameter and cannot use that path, but they
/// have to look identical: a panel where one row is drawn differently from the
/// thirty above it reads as a bug.
///
/// The in-flight value is held here rather than read back from wherever the
/// number lives, so the document is not diffed sixty times a second while a
/// finger is down. Bracketing the drag into one undo step is the *caller's*
/// business, because only it knows what the drag is of — hence `onBegin` and
/// `onEnd` rather than a store reference.
public struct ScalarRow: View {
    let name: String
    let unit: String
    let value: Float
    let bounds: Bounds
    let isActive: Bool
    /// Called on every frame of a drag, and once for the reset arrow — which
    /// is a discrete change and its own undo step, so it arrives outside any
    /// `onBegin`/`onEnd` pair.
    let onChange: (Float) -> Void
    let onBegin: () -> Void
    let onEnd: () -> Void

    @State private var dragging: Float?

    public init(
        name: String, unit: String, value: Float, bounds: Bounds, isActive: Bool,
        onChange: @escaping (Float) -> Void,
        onBegin: @escaping () -> Void,
        onEnd: @escaping () -> Void
    ) {
        self.name = name
        self.unit = unit
        self.value = value
        self.bounds = bounds
        self.isActive = isActive
        self.onChange = onChange
        self.onBegin = onBegin
        self.onEnd = onEnd
    }

    private var shown: Float { dragging ?? value }

    public var body: some View {
        HStack(spacing: RowMetrics.gap) {
            Text(name)
                .frame(width: RowMetrics.label, alignment: .trailing)
                .lineLimit(1)
                .foregroundStyle(isActive ? .primary : .tertiary)

            track

            Text(readout)
                .frame(width: RowMetrics.value, alignment: .trailing)
                .monospacedDigit()
                .foregroundStyle(isActive ? .primary : .tertiary)

            Button {
                onChange(bounds.neutral)
            } label: {
                Image(systemName: "arrow.uturn.backward")
                    .imageScale(.small)
            }
            .buttonStyle(.borderless)
            .frame(width: RowMetrics.reset)
            .help("Back to \(format(bounds.neutral))")
        }
        .frame(height: RowMetrics.height)
        .disabled(!isActive)
    }

    private var readout: String {
        unit.isEmpty ? format(shown) : "\(format(shown)) \(unit)"
    }

    private func format(_ v: Float) -> String {
        // A temperature in kelvin has no useful fraction; an exposure in stops
        // is nothing but fraction.
        abs(bounds.max - bounds.min) > 100
            ? String(format: "%.0f", v)
            : String(format: "%.2f", v)
    }

    private var track: some View {
        GeometryReader { geo in
            let g = SliderGeometry(bounds: bounds, width: geo.size.width)
            let fill = g.fill(for: shown)
            ZStack(alignment: .leading) {
                Capsule()
                    .fill(.quaternary)
                    .frame(height: 3)
                Rectangle()
                    .fill(.tint)
                    .frame(width: fill.width, height: 3)
                    .offset(x: fill.origin)
                Circle()
                    .fill(.primary)
                    .frame(width: 9, height: 9)
                    .offset(x: g.position(of: shown) - 4.5)
            }
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        if dragging == nil {
                            onBegin()
                        }
                        let v = g.value(at: drag.location.x)
                        dragging = v
                        onChange(v)
                    }
                    .onEnded { _ in
                        onEnd()
                        dragging = nil
                    }
            )
        }
    }
}

/// One float parameter, as a draggable track.
///
/// The drag is bracketed so it becomes one undo step: `beginInteraction` on
/// the way down, one engine call per frame, `endInteraction` on the way up.
///
/// Nothing but the wiring: the drawing is `ScalarRow`'s, so a pin's controls
/// and a registry parameter's cannot drift apart.
public struct FloatRow: View {
    let effectName: String
    let param: Param
    let bounds: Bounds
    let row: UInt64
    let value: Float
    let isActive: Bool
    let store: SessionStore

    public init(
        effectName: String, param: Param, bounds: Bounds, row: UInt64,
        value: Float, isActive: Bool, store: SessionStore
    ) {
        self.effectName = effectName
        self.param = param
        self.bounds = bounds
        self.row = row
        self.value = value
        self.isActive = isActive
        self.store = store
    }

    public var body: some View {
        ScalarRow(
            name: param.name, unit: param.unit, value: value, bounds: bounds,
            isActive: isActive,
            onChange: { store.setFloat(row: row, key: param.key, value: $0) },
            onBegin: { store.beginInteraction(param.name) },
            onEnd: { store.endInteraction() }
        )
    }
}
