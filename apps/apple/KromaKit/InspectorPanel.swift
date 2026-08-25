import SwiftUI

/// One effect's controls, generated from its registry entry.
///
/// Nothing here knows what an exposure is. All eight parameter kinds have a
/// control, so the whole registry is covered — including effects added later,
/// with no Swift changes at all. What remains is the `.opaque` fallback, for a
/// kind a future version adds and this build has never heard of.
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

            if !wheels.isEmpty {
                HStack(alignment: .top, spacing: 4) {
                    ForEach(wheels) { param in
                        wheel(param)
                    }
                }
                .padding(.bottom, 4)
            }

            if !curves.isEmpty {
                CurvePanel(effect: effect, params: curves, row: row, store: store)
            }

            if !warpSections.isEmpty {
                WarperPanel(effect: effect, sections: warpSections, row: row, store: store)
            }

            ForEach(effect.params.filter {
                !isWheel($0) && !isCurve($0) && !warpSections.contains($0.section)
            }) { param in
                control(for: param)
            }
        }
        .padding(.vertical, 6)
    }

    private func isWheel(_ param: Param) -> Bool {
        if case .wheel = param.kind { return true }
        return false
    }

    private var wheels: [Param] {
        effect.params.filter(isWheel)
    }

    private func isCurve(_ param: Param) -> Bool {
        if case .curve = param.kind { return true }
        return false
    }

    private var curves: [Param] {
        effect.params.filter(isCurve)
    }

    private func isWarp(_ param: Param) -> Bool {
        if case .warp = param.kind { return true }
        return false
    }

    private func isPins(_ param: Param) -> Bool {
        if case .pins = param.kind { return true }
        return false
    }

    /// Sections that contain a lattice or a set of pins, in registry order and
    /// without repeats.
    ///
    /// Pins belong here rather than in the flat list because Chroma Warp is a
    /// third view of the Colour Warper, not a control sitting beside it — and
    /// registry order is what puts the tabs in Resolve's order, so this must
    /// not sort.
    ///
    /// A lattice with an *empty* section is claimed by no tab and would fall
    /// through to the flat list. No registered effect has one; if one appears
    /// it belongs in the panel, not in the list.
    private var warpSections: [String] {
        var seen = Set<String>()
        return effect.params
            .filter { (isWarp($0) || isPins($0)) && !$0.section.isEmpty }
            .map(\.section)
            .filter { seen.insert($0).inserted }
    }

    @ViewBuilder
    private func wheel(_ param: Param) -> some View {
        if case let .wheel(bounds, master) = param.kind {
            WheelView(
                param: param,
                bounds: bounds,
                hasMaster: master,
                row: row.id,
                value: row.params[param.key]?.wheelValue
                    ?? WheelValue(rgb: [bounds.default, bounds.default, bounds.default],
                                  master: bounds.default),
                isActive: effect.isActive(param.key, values: row.params),
                store: store
            )
        }
    }

    @ViewBuilder
    private func control(for param: Param) -> some View {
        Self.control(for: param, effect: effect, row: row, store: store)
    }

    /// One parameter's row, chosen by kind.
    ///
    /// Static, and taking its context explicitly, because `WarperPanel` draws
    /// the rows of the section it has claimed and must draw them exactly as
    /// the inspector does. A second copy of this `switch` would be two
    /// switches drifting apart, and a parameter kind added to one of them.
    @ViewBuilder
    static func control(
        for param: Param, effect: Effect, row: Snapshot.Row, store: SessionStore
    ) -> some View {
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
            // A kind this build does not draw. Nothing the current registry
            // declares reaches this arm — every kind now has a control — but a
            // document written by a later version can carry one this build has
            // never heard of, and a control that silently is not there is a
            // parameter the user cannot reach and is not told about.
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
