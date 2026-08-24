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

/// One float parameter, as a draggable track.
///
/// The drag is bracketed so it becomes one undo step: `beginInteraction` on
/// the way down, one engine call per frame, `endInteraction` on the way up.
/// The in-flight value is held here rather than read back from the snapshot,
/// so the document is not diffed sixty times a second while a finger is down.
public struct FloatRow: View {
    let effectName: String
    let param: Param
    let bounds: Bounds
    let row: UInt64
    let value: Float
    let isActive: Bool
    let store: SessionStore

    @State private var dragging: Float?

    private var shown: Float { dragging ?? value }

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
        HStack(spacing: RowMetrics.gap) {
            Text(param.name)
                .frame(width: RowMetrics.label, alignment: .trailing)
                .lineLimit(1)
                .foregroundStyle(isActive ? .primary : .tertiary)

            track

            Text(readout)
                .frame(width: RowMetrics.value, alignment: .trailing)
                .monospacedDigit()
                .foregroundStyle(isActive ? .primary : .tertiary)

            Button {
                commit(bounds.neutral)
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
        param.unit.isEmpty ? format(shown) : "\(format(shown)) \(param.unit)"
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
                            store.beginInteraction(param.name)
                        }
                        let v = g.value(at: drag.location.x)
                        dragging = v
                        store.setFloat(row: row, key: param.key, value: v)
                    }
                    .onEnded { _ in
                        store.endInteraction()
                        dragging = nil
                    }
            )
        }
    }

    /// A discrete change — the reset arrow — which is its own undo step rather
    /// than part of whatever drag came before it.
    private func commit(_ v: Float) {
        store.setFloat(row: row, key: param.key, value: v)
    }
}
