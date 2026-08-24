import SwiftUI

struct ContentView: View {
    let store: SessionStore

    var body: some View {
        VStack(spacing: 0) {
            MetalViewer(store: store)
                .frame(minWidth: 480, minHeight: 320)
            statusBar
        }
        .frame(minWidth: 720, minHeight: 480)
        .onAppear {
            store.setSupportDirectory(Self.supportDirectory)
            store.openTestChart()
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
                Text("\(store.snapshot.width)x\(store.snapshot.height)")
                    .foregroundStyle(.secondary)
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
