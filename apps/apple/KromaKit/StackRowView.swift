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

    /// What Blend measures over. Neutral at the top, because a row that is
    /// fully applied is a row doing nothing unusual — so the fill grows
    /// *leftwards* as it is dialled back, which is the direction the change is.
    static let blend = Bounds(min: 0, max: 1, default: 1, neutral: 1)

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
            header

            // Blend, as the same row every other parameter is: the same track,
            // the same pointer, the same boxed number that can be dragged at a
            // quarter speed or typed into. It was a capsule with a disc on it,
            // which is the one row in the panel drawn unlike the thirty under
            // it — and that reads as a bug rather than as a distinction.
            ScalarRow(
                name: "Blend",
                unit: "",
                value: row.opacity,
                bounds: Self.blend,
                isActive: row.enabled,
                onChange: { store.setRowOpacity(row.id, $0) },
                onBegin: { store.beginInteraction("Opacity") },
                onEnd: { store.endInteraction() }
            )
        }
    }

    /// The row's own header: what it is, whether it is doing anything, and the
    /// buttons that move or remove it.
    ///
    /// On `RAISED`, one step up from the panel it sits on — which is the job
    /// that grey was chosen for, and what separates one effect's controls from
    /// the next one's without a second rule.
    private var header: some View {
        HStack(spacing: 4) {
            Toggle("", isOn: Binding(
                get: { row.enabled },
                set: { store.setRowEnabled(row.id, $0) }
            ))
            .labelsHidden()
            .toggleStyle(KromaCheckboxStyle())
            .help("Bypass this row")

            // The accent, and the only thing wearing it here. A bypassed row
            // is doing nothing, so it is dimmed rather than recoloured.
            Text(effect.name)
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(Palette.accent.color)
                .opacity(row.enabled ? 1 : ScalarRow.dimmed)
                .lineLimit(1)

            Spacer()

            IconButton("chevron.up", help: "Move up") {
                store.moveRow(row.id, to: UInt32(max(floor, index - 1)))
            }
            .disabled(index <= floor)

            IconButton("chevron.down", help: "Move down") {
                store.moveRow(row.id, to: UInt32(min(count - 1, index + 1)))
            }
            .disabled(index >= count - 1)

            if store.canRemove(row) {
                IconButton("trash", help: "Remove this row") {
                    store.removeRow(row.id)
                }
            }
        }
        .padding(.horizontal, 4)
        .frame(height: RowMetrics.height)
        .background(Palette.raised.color)
    }
}
