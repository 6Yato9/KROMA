import SwiftUI

struct ContentView: View {
    let store: SessionStore

    var body: some View {
        HSplitView {
            VStack(spacing: 0) {
                MetalViewer(store: store)
                    .frame(minWidth: 480, minHeight: 320)
                statusBar
            }
            inspector
                .frame(width: 260)
        }
        .frame(minWidth: 900, minHeight: 560)
        .onAppear {
            store.setSupportDirectory(Self.supportDirectory)
            store.openTestChart()
        }
    }

    /// The pinned rows, in pinned order, each generated from the registry.
    private var inspector: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(store.registry.pinnedEffects) { effect in
                    if let row = store.snapshot.rows.first(where: {
                        $0.effect == effect.key && $0.pinned
                    }) {
                        InspectorPanel(effect: effect, row: row, store: store)
                        Divider()
                    }
                }
            }
            .padding(.horizontal, 8)
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
