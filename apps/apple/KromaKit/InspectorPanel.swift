import SwiftUI

/// One effect's controls, generated from its registry entry.
///
/// Nothing here knows what an exposure is. Every effect that declares only
/// float parameters is already fully rendered by this; the remaining seven
/// parameter kinds each need one view, after which the whole registry is
/// covered — including effects added later, with no Swift changes at all.
public struct InspectorPanel: View {
    let effect: Effect
    let row: Snapshot.Row
    let store: SessionStore

    public init(effect: Effect, row: Snapshot.Row, store: SessionStore) {
        self.effect = effect
        self.row = row
        self.store = store
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(effect.name)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.bottom, 2)

            ForEach(effect.params) { param in
                control(for: param)
            }
        }
        .padding(.vertical, 6)
    }

    @ViewBuilder
    private func control(for param: Param) -> some View {
        switch param.kind {
        case let .float(bounds):
            FloatRow(
                effectName: effect.name,
                param: param,
                bounds: bounds,
                row: row.id,
                value: row.params[param.key]?.floatValue ?? bounds.default,
                isActive: effect.isActive(param.key, values: row.params),
                store: store
            )

        case let .bool(defaultValue):
            BoolRow(
                param: param,
                row: row.id,
                value: {
                    if case let .bool(v) = row.params[param.key] { return v }
                    return defaultValue
                }(),
                isActive: effect.isActive(param.key, values: row.params),
                store: store
            )

        case let .choice(options, defaultValue):
            ChoiceRow(
                param: param,
                options: options,
                row: row.id,
                value: {
                    if case let .choice(v) = row.params[param.key] { return v }
                    return defaultValue
                }(),
                isActive: effect.isActive(param.key, values: row.params),
                store: store
            )

        case let .rgb(defaultValue):
            RgbRow(
                param: param,
                row: row.id,
                value: {
                    if case let .rgb(v) = row.params[param.key] { return v }
                    return defaultValue
                }(),
                isActive: effect.isActive(param.key, values: row.params),
                store: store
            )
        default:
            // A kind this slice does not draw yet. Named rather than skipped:
            // a control that silently is not there is a parameter the user
            // cannot reach and is not told about.
            HStack(spacing: RowMetrics.gap) {
                Text(param.name)
                    .frame(width: RowMetrics.label, alignment: .trailing)
                    .lineLimit(1)
                Text("not yet")
                    .foregroundStyle(.tertiary)
                Spacer()
            }
            .font(.caption)
            .frame(height: RowMetrics.height)
        }
    }
}
