import SwiftUI

/// One added row: what it is, whether it is doing anything, how much of it, and
/// the buttons that move or remove it.
///
/// Every row carries its own `enabled`, `opacity` and `blend`, mirroring a
/// Resolve node's anatomy — so "grain at forty per cent in Screen mode" is
/// three fields that already exist rather than a feature built into grain.
public struct StackRowView: View {
    let effect: Effect
    let row: Snapshot.Row
    let index: Int
    let count: Int
    /// The first index an added row can occupy. The pinned rows hold the top
    /// of the stack and nothing may move above them — `Stack::reorder` clamps
    /// to it and returns quietly, so without this the arrow is enabled, does
    /// nothing, and reports nothing.
    let floor: Int
    let store: SessionStore

    @State private var dragging: Float?

    public init(
        effect: Effect, row: Snapshot.Row, index: Int, count: Int, floor: Int,
        store: SessionStore
    ) {
        self.effect = effect
        self.row = row
        self.index = index
        self.count = count
        self.floor = floor
        self.store = store
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 4) {
                Toggle("", isOn: Binding(
                    get: { row.enabled },
                    set: { store.setRowEnabled(row.id, $0) }
                ))
                .labelsHidden()
                .toggleStyle(.checkbox)
                .help("Bypass this row")

                Text(effect.name)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(row.enabled ? .secondary : .tertiary)

                Spacer()

                Button {
                    store.moveRow(row.id, to: UInt32(max(floor, index - 1)))
                } label: {
                    Image(systemName: "chevron.up")
                }
                .buttonStyle(.borderless)
                .disabled(index <= floor)

                Button {
                    store.moveRow(row.id, to: UInt32(min(count - 1, index + 1)))
                } label: {
                    Image(systemName: "chevron.down")
                }
                .buttonStyle(.borderless)
                .disabled(index >= count - 1)

                if store.canRemove(row) {
                    Button(role: .destructive) {
                        store.removeRow(row.id)
                    } label: {
                        Image(systemName: "trash")
                    }
                    .buttonStyle(.borderless)
                }
            }

            // Opacity, as the same four-column row every parameter uses, so the
            // columns line up with the controls underneath it.
            HStack(spacing: RowMetrics.gap) {
                Text("Blend")
                    .frame(width: RowMetrics.label, alignment: .trailing)
                    .foregroundStyle(.tertiary)
                GeometryReader { geo in
                    let bounds = Bounds(min: 0, max: 1, default: 1, neutral: 1)
                    let g = SliderGeometry(bounds: bounds, width: geo.size.width)
                    let shown = dragging ?? row.opacity
                    ZStack(alignment: .leading) {
                        Capsule().fill(.quaternary).frame(height: 3)
                        Circle()
                            .fill(.primary)
                            .frame(width: 8, height: 8)
                            .offset(x: g.position(of: shown) - 4)
                    }
                    .frame(maxHeight: .infinity)
                    .contentShape(Rectangle())
                    .gesture(
                        DragGesture(minimumDistance: 0)
                            .onChanged { drag in
                                if dragging == nil { store.beginInteraction("Opacity") }
                                let v = g.value(at: drag.location.x)
                                dragging = v
                                store.setRowOpacity(row.id, v)
                            }
                            .onEnded { _ in
                                store.endInteraction()
                                dragging = nil
                            }
                    )
                }
                Text(String(format: "%.0f%%", (dragging ?? row.opacity) * 100))
                    .frame(width: RowMetrics.value, alignment: .trailing)
                    .monospacedDigit()
                    .foregroundStyle(.tertiary)
            }
            .frame(height: RowMetrics.height)
            .font(.caption)
        }
    }
}
