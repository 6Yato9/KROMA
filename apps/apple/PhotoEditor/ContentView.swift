import SwiftUI

struct ContentView: View {
    let store: SessionStore

    /// Whether the scopes panel is open. Off to begin with: it is a full extra
    /// render and a 1.2 MB readback per edit, and somebody who has not asked
    /// for it should not be paying for it.
    @AppStorage("showScopes") private var showScopes = false

    var body: some View {
        HSplitView {
            VStack(spacing: 0) {
                viewerAndScopes
                statusBar
            }
            inspector
                // Wide enough for a row, and resizable. It was pinned at 260,
                // which is less than the label, readout and reset arrow cost
                // between them — every control in the application was drawing
                // its label with the front clipped off.
                .frame(
                    minWidth: RowMetrics.minimumPanel,
                    idealWidth: 330,
                    maxWidth: 520
                )
        }
        .frame(minWidth: 900, minHeight: 560)
        .onAppear {
            store.setSupportDirectory(Self.supportDirectory)
            store.openTestChart()
        }
    }

    /// The photograph, with the scopes under it when they are asked for.
    ///
    /// A split rather than a fixed height, because how much of the window a
    /// colourist gives the scopes is the sort of thing they change per
    /// photograph — and it is how the inspector is already divided from the
    /// picture.
    @ViewBuilder
    private var viewerAndScopes: some View {
        if showScopes {
            VSplitView {
                MetalViewer(store: store)
                    .frame(minWidth: 480, minHeight: 200)
                ScopesPanel(store: store)
                    .frame(minHeight: 140, idealHeight: 240, maxHeight: .infinity)
            }
        } else {
            MetalViewer(store: store)
                .frame(minWidth: 480, minHeight: 320)
        }
    }

    /// The pinned panels, then everything that has been added, then the button
    /// that adds more.
    private var inspector: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(store.snapshot.rows.enumerated()), id: \.element.id) { index, row in
                    if let effect = store.registry.effect(row.effect) {
                        if !row.pinned {
                            StackRowView(
                                effect: effect,
                                row: row,
                                index: index,
                                count: store.snapshot.rows.count,
                                floor: store.snapshot.rows.filter(\.pinned).count,
                                store: store
                            )
                        }
                        InspectorPanel(effect: effect, row: row, store: store)
                        Divider()
                    }
                }

                EffectBrowser(registry: store.registry, store: store)
                    .padding(.vertical, 8)
            }
            .padding(.horizontal, RowMetrics.inset)
        }
    }

    /// `~/Library/Application Support/Kroma`, which is where a Mac application
    /// keeps what belongs to it. The engine does not guess this; it is told.
    static var supportDirectory: URL {
        let base = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask
        ).first ?? FileManager.default.temporaryDirectory
        return base.appendingPathComponent("Kroma", isDirectory: true)
    }

    /// The passes counter, which is the number worth watching: with a deep
    /// stack, dragging the deepest slider should read 1.
    private var statusBar: some View {
        HStack(spacing: 12) {
            if let problem = store.problem {
                Text(problem)
                    .foregroundStyle(.red)
                    .lineLimit(1)
                    .help(problem)
            } else if store.snapshot.isOpen {
                Text(store.snapshot.name ?? "test chart")
                    .foregroundStyle(.secondary)
                Text("\(store.snapshot.width)x\(store.snapshot.height)")
                    .foregroundStyle(.tertiary)
            }
            Spacer()
            Toggle("Scopes", isOn: $showScopes)
                .toggleStyle(.button)
                .controlSize(.small)
                .help("Waveform, parade, vectorscope and histogram")
            Text("passes \(store.snapshot.passes)")
                .foregroundStyle(.secondary)
                .monospacedDigit()
        }
        .font(.caption)
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(.bar)
    }
}
