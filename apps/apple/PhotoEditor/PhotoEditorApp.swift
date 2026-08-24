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
}
