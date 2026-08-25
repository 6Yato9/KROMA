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
    /// Whether to draw the effect's own name.
    ///
    /// An added row already has one: `StackRowView` is its header, and the
    /// name belongs beside the bypass box and the arrows that act on it. The
    /// pinned rows have no such header, so this is where their name is drawn.
    /// Two of them, one under the other, is what the panel did before.
    let showsTitle: Bool

    public init(
        effect: Effect, row: Snapshot.Row, store: SessionStore, showsTitle: Bool = true
    ) {
        self.effect = effect
        self.row = row
        self.store = store
        self.showsTitle = showsTitle
    }

    public var body: some View {
        if showsTitle {
            // Foldable, and titled in the accent only while it is open.
            // Resolve spends the accent on the effect you are working in and
            // nowhere else; seven pinned panels shouting it at once is the
            // same nothing as accenting every heading. Folding is also what
            // makes a column of nine panels navigable at all.
            InspectorSection(effect: effect.key, title: effect.name, namesAnEffect: true) {
                contents
            }
        } else {
            contents
        }
    }

    /// Everything the panel draws under its own name.
    @ViewBuilder
    private var contents: some View {
        VStack(alignment: .leading, spacing: 2) {
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

            ForEach(blocks) { block in
                switch block {
                case let .loose(param):
                    control(for: param)
                case let .section(name, params):
                    InspectorSection(effect: effect.key, title: name) {
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(params) { param in
                                control(for: param)
                            }
                        }
                    }
                }
            }
        }
        .padding(.vertical, 6)
    }

    /// The rows that are neither a wheel, a curve, nor claimed by the warper's
    /// own switcher.
    var loose: [Param] { Self.loose(of: effect) }

    /// The same, from an effect alone, so what the panel lays out can be asked
    /// without a session and a document behind it.
    static func loose(of effect: Effect) -> [Param] {
        let warped = warpSections(of: effect)
        return effect.params.filter {
            !isWheel($0) && !isCurve($0) && !warped.contains($0.section)
        }
    }

    /// One run of the panel: a row at the top level, or a section with its
    /// rows inside it.
    ///
    /// The registry gives every parameter a `section` and this panel ignored
    /// it, which is why an effect with thirty controls was thirty rows in one
    /// column with nothing to say where one group of them ended.
    enum Block: Identifiable {
        case loose(Param)
        case section(String, [Param])

        var id: String {
            switch self {
            case let .loose(param): "param:\(param.key)"
            case let .section(name, _): "section:\(name)"
            }
        }
    }

    /// The panel's runs, in registry order.
    ///
    /// Order is the registry's throughout — a section takes the position of
    /// its *first* parameter, and a sectionless one keeps its own. Sorting, or
    /// hoisting every section below every loose row, would put Resolve's
    /// controls in an order Resolve does not use, and the registry is where
    /// that order is decided.
    var blocks: [Block] { Self.blocks(of: loose) }

    static func blocks(of rows: [Param]) -> [Block] {
        var out: [Block] = []
        var seen = Set<String>()
        for param in rows {
            if param.section.isEmpty {
                out.append(.loose(param))
                continue
            }
            guard seen.insert(param.section).inserted else { continue }
            out.append(.section(param.section, rows.filter { $0.section == param.section }))
        }
        return out
    }

    static func isWheel(_ param: Param) -> Bool {
        if case .wheel = param.kind { return true }
        return false
    }

    private var wheels: [Param] {
        effect.params.filter(Self.isWheel)
    }

    static func isCurve(_ param: Param) -> Bool {
        if case .curve = param.kind { return true }
        return false
    }

    private var curves: [Param] {
        effect.params.filter(Self.isCurve)
    }

    static func isWarp(_ param: Param) -> Bool {
        if case .warp = param.kind { return true }
        return false
    }

    static func isPins(_ param: Param) -> Bool {
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
    private var warpSections: [String] { Self.warpSections(of: effect) }

    static func warpSections(of effect: Effect) -> [String] {
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
                    .foregroundStyle(Palette.label.color)
                    .frame(width: RowMetrics.label, alignment: .trailing)
                    .lineLimit(1)
                Text("not yet")
                    .foregroundStyle(Palette.dim.color)
                Spacer()
            }
            .font(.system(size: 11.5))
            .frame(height: RowMetrics.height)
        }
    }
}

/// A collapsible heading, drawn as `resolve.rs::section` draws one: a `RULE`
/// hairline along the top, a chevron, and the title in `TITLE` at twelve
/// points.
///
/// No accent, deliberately. Resolve titles the *open effect* in its accent and
/// spends it nowhere else; a heading in it as well, twice per effect, takes the
/// one loud colour in an otherwise grey scheme and makes it mean nothing.
///
/// Collapsible and *remembered*, because thirty parameters in one column is the
/// reason Resolve made these fold at all — and a panel that forgot what you had
/// folded would unfold it again on the next snapshot, which is sixty times a
/// second while a slider is moving.
struct InspectorSection<Content: View>: View {
    let title: String
    /// Whether this heading names a whole effect rather than a group inside
    /// one. An effect's name is accented while its panel is open and grey
    /// while it is shut, which is what `resolve.rs` does — and the reason the
    /// accent stays worth something. Seven pinned panels all titled in it at
    /// once says exactly as little as accenting every heading would.
    let namesAnEffect: Bool
    private let content: () -> Content
    @AppStorage private var open: Bool

    init(
        effect: String,
        title: String,
        namesAnEffect: Bool = false,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.title = title
        self.namesAnEffect = namesAnEffect
        self.content = content
        // Keyed by effect as well as by section: "Add Vignetting" folded away
        // under one effect says nothing about the section of the same name
        // under another.
        _open = AppStorage(
            wrappedValue: true,
            namesAnEffect ? "effect.\(effect)" : "section.\(effect).\(title)"
        )
    }

    /// What the title is drawn in. Only an *open* effect gets the accent.
    ///
    /// A free function so the rule can be asserted without standing a view up
    /// and driving its stored state — the rule is the point, not the plumbing.
    static func titleColour(namesAnEffect: Bool, open: Bool) -> Color {
        namesAnEffect && open ? Palette.accent.color : Palette.title.color
    }

    var titleColour: Color {
        Self.titleColour(namesAnEffect: namesAnEffect, open: open)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            header
            if open {
                content()
            }
        }
    }

    private var header: some View {
        Button {
            open.toggle()
        } label: {
            HStack(spacing: 0) {
                Chevron(open: open)
                    .stroke(
                        Palette.icon.color,
                        style: StrokeStyle(lineWidth: 1.4, lineCap: .round, lineJoin: .round)
                    )
                    // The Windows shell puts the chevron's centre ten points
                    // in and the title's baseline at twenty-two.
                    .frame(width: 7.2, height: 7.2)
                    .padding(.leading, 6.4)
                    .padding(.trailing, 8.4)

                Text(title)
                    .font(.system(size: 12, weight: namesAnEffect ? .semibold : .regular))
                    .foregroundStyle(titleColour)
                    .lineLimit(1)

                Spacer(minLength: 0)
            }
            .frame(height: RowMetrics.height)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .overlay(alignment: .top) { Hairline() }
    }
}
