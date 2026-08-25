import SwiftUI

/// One of an effect's enumerated options.
///
/// The options come from the registry, so a choice added in Rust appears here
/// with nothing written on this side.
public struct ChoiceRow: View {
    let param: Param
    let options: [String]
    let row: UInt64
    let value: String
    let isActive: Bool
    let store: SessionStore

    public init(
        param: Param, options: [String], row: UInt64, value: String,
        isActive: Bool, store: SessionStore
    ) {
        self.param = param
        self.options = options
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

            ChoiceMenu(options: options, chosen: value) {
                store.setChoice(row: row, key: param.key, value: $0)
            }
            .frame(maxWidth: 140)

            Spacer()
        }
        .frame(height: RowMetrics.height)
        .opacity(isActive ? 1 : ScalarRow.dimmed)
        .disabled(!isActive)
    }
}
