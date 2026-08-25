import SwiftUI

/// A switch, in the same four-column row as everything else.
///
/// Its own file rather than another arm of a growing switch statement, because
/// there will be eight of these and a single file holding all of them is the
/// thing `inspector.rs` avoided on the Windows side.
public struct BoolRow: View {
    let param: Param
    let row: UInt64
    let value: Bool
    let isActive: Bool
    let store: SessionStore

    public init(param: Param, row: UInt64, value: Bool, isActive: Bool, store: SessionStore) {
        self.param = param
        self.row = row
        self.value = value
        self.isActive = isActive
        self.store = store
    }

    public var body: some View {
        HStack(spacing: RowMetrics.gap) {
            Text(param.name)
                .font(.system(size: 11.5))
                .foregroundStyle(Palette.label.color)
                .frame(width: RowMetrics.label, alignment: .trailing)
                .lineLimit(1)

            Toggle("", isOn: Binding(
                get: { value },
                set: { store.setBool(row: row, key: param.key, value: $0) }
            ))
            .labelsHidden()
            .toggleStyle(KromaCheckboxStyle())

            Spacer()
        }
        .frame(height: RowMetrics.height)
        // The same dim every other row uses. `.disabled` alone fades SwiftUI's
        // semantic styles, and nothing here is one of those any more.
        .opacity(isActive ? 1 : ScalarRow.dimmed)
        .disabled(!isActive)
    }
}
