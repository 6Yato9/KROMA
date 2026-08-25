import SwiftUI

/// The sections of an effect that contain a lattice, behind a switcher.
///
/// Resolve puts each grid's divisions controls under the view they belong to
/// rather than in a flat list beside every other one, and the registry already
/// carries that grouping: every parameter has a `section`. So a section that
/// contains a lattice is drawn here — its lattice and its other rows together
/// — and the switcher runs over sections rather than over a hard-coded list of
/// views. Any later effect that declares a `Warp` gets the same treatment with
/// no Swift changes.
public struct WarperPanel: View {
    let effect: Effect
    /// Section names, in registry order, each containing at least one lattice.
    let sections: [String]
    let row: Snapshot.Row
    let store: SessionStore

    @State private var chosen: String?
    /// Which lattice, for a section that has more than one. Grid 1 and Grid 2
    /// are not two halves of a control: they are the same warp about two
    /// different chromaticity axes, and Axis Angle is what separates them.
    @State private var whichLattice: Int = 0

    public init(effect: Effect, sections: [String], row: Snapshot.Row, store: SessionStore) {
        self.effect = effect
        self.sections = sections
        self.row = row
        self.store = store
    }

    private var section: String { chosen ?? sections.first ?? "" }

    private func params(in section: String) -> [Param] {
        effect.params.filter { $0.section == section }
    }

    private func isLattice(_ param: Param) -> Bool {
        if case .warp = param.kind { return true }
        return false
    }

    private func lattices(in section: String) -> [Param] {
        params(in: section).filter(isLattice)
    }

    /// Hue wraps; chroma does not. Read from the section's own lattice keys
    /// rather than from the section name, which is a label and could be
    /// translated.
    private func axes(for param: Param) -> WarpAxes {
        param.key == "hue_sat" ? .hueSat : .chromaLuma
    }

    /// The lattice being shown, if the current section has one at all.
    private var current: Param? {
        let grids = lattices(in: section)
        return grids.indices.contains(whichLattice) ? grids[whichLattice] : grids.first
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            sectionPicker
            latticePicker
            if let param = current {
                editor(param)
            }

            // The section's own rows — its divisions, and Axis Angle — under
            // the grid they govern, which is where Resolve puts them.
            ForEach(params(in: section).filter { !isLattice($0) }) { param in
                InspectorPanel.control(for: param, effect: effect, row: row, store: store)
            }
        }
        .padding(.bottom, 4)
    }

    @ViewBuilder
    private var sectionPicker: some View {
        if sections.count > 1 {
            Picker("", selection: Binding(
                get: { section },
                set: { chosen = $0; whichLattice = 0 }
            )) {
                ForEach(sections, id: \.self) { Text($0).tag($0) }
            }
            .labelsHidden()
            .pickerStyle(.segmented)
            .controlSize(.small)
        }
    }

    @ViewBuilder
    private var latticePicker: some View {
        let grids = lattices(in: section)
        if grids.count > 1 {
            Picker("", selection: $whichLattice) {
                ForEach(Array(grids.enumerated()), id: \.offset) { i, p in
                    Text(p.name).tag(i)
                }
            }
            .labelsHidden()
            .pickerStyle(.segmented)
            .controlSize(.small)
        }
    }

    @ViewBuilder
    private func editor(_ param: Param) -> some View {
        WarpEditor(
            param: param,
            axes: axes(for: param),
            row: row.id,
            value: row.params[param.key]?.warpValue
                ?? WarpValue(cols: 6, rows: 6,
                             offsets: Array(repeating: .zero, count: 36)),
            isActive: effect.isActive(param.key, values: row.params),
            store: store
        )

        Button("Reset \(param.name)") {
            store.clearWarp(row: row.id, key: param.key)
        }
        .buttonStyle(.borderless)
        .controlSize(.small)
    }
}
