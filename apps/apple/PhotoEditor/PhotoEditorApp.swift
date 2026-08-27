import AppKit
import SwiftUI
import UniformTypeIdentifiers

@main
struct PhotoEditorApp: App {
    @State private var store = SessionStore()

    var body: some Scene {
        WindowGroup {
            if let store {
                ContentView(store: store)
                    // Across the foot of the whole window rather than inside
                    // one panel: a run belongs to the session, not to the
                    // inspector or the strip. It draws nothing at all when
                    // there is no run and nothing to report.
                    .safeAreaInset(edge: .bottom, spacing: 0) {
                        BatchProgress(store: store)
                    }
                    .onDisappear { store.flush() }
            } else {
                // The engine failed to start, which on a Mac means no Metal
                // device. Saying so beats an empty window.
                Text("The engine could not start.")
                    .padding(40)
            }
        }
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("Open…") { open() }
                    .keyboardShortcut("o", modifiers: .command)
            }
            CommandGroup(replacing: .undoRedo) {
                Button("Undo") { store?.undo() }
                    .keyboardShortcut("z", modifiers: .command)
                    .disabled(!(store?.canUndo ?? false))
                Button("Redo") { store?.redo() }
                    .keyboardShortcut("z", modifiers: [.command, .shift])
                    .disabled(!(store?.canRedo ?? false))
            }
            CommandGroup(replacing: .saveItem) {
                Button("Export") { store?.export() }
                    .keyboardShortcut("e", modifiers: .command)
                    .disabled(!(store?.snapshot.isOpen ?? false))
                Button("Export All…") { exportAll() }
                    .keyboardShortcut("e", modifiers: [.command, .shift])
                    .disabled(!(store?.canStartBatch ?? false))
                Button("Revert") { store?.revert() }
                    .disabled(!(store?.snapshot.isOpen ?? false))
            }
        }
    }

    /// The engine works in paths, and making a path valid is the host's job.
    /// Unsandboxed today, so a path from the panel is already valid; the
    /// bookmark-shaped version of this arrives with the sandbox and with iOS.
    private func open() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.jpeg, .png]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        store?.open(url)
    }

    /// Where a batch writes: a folder, chosen.
    ///
    /// Rather than beside each original, because a batch written back into the
    /// folder it read would be the next run's input. The engine refuses to land
    /// on one of the photographs it was given whatever is picked here, and
    /// counts that as one photograph missed rather than as the end of the run.
    ///
    /// The run itself is a step a frame from the display link; nothing is
    /// exported on this button's own thread beyond choosing where.
    private func exportAll() {
        guard let store, let first = store.library.entries.first else { return }
        let panel = NSOpenPanel()
        panel.title = "Export all to"
        panel.prompt = "Export"
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        // Beside the photographs is where somebody starts looking for the
        // folder they mean, even though it is not where the exports should go.
        panel.directoryURL = first.path.deletingLastPathComponent()
        guard panel.runModal() == .OK, let url = panel.url else { return }
        store.startBatch(into: url)
    }
}
