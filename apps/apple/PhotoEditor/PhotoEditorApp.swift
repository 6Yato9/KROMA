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
                Button("Open Folder…") { openFolder() }
                    .keyboardShortcut("o", modifiers: [.command, .shift])
            }
            CommandGroup(replacing: .undoRedo) {
                Button("Undo") { store?.undo() }
                    .keyboardShortcut("z", modifiers: .command)
                    .disabled(!(store?.canUndo ?? false))
                Button("Redo") { store?.redo() }
                    .keyboardShortcut("z", modifiers: [.command, .shift])
                    .disabled(!(store?.canRedo ?? false))
            }
            // The Grade menu, which `main.rs` has beside File and Export.
            //
            // Its own menu rather than items under Edit: copying a *grade* and
            // copying a *selection* are different verbs, and putting them on
            // the same Cmd-C would mean the shortcut did different things
            // depending on where the pointer happened to be.
            CommandMenu("Grade") {
                Button("Copy") { store?.copyGrade() }
                    .disabled(!(store?.snapshot.isOpen ?? false))
                Button("Paste") { store?.pasteGrade() }
                    .disabled(!(store?.hasGrade ?? false))
                Button("Paste to All…") { store?.pasteGradeToAll() }
                    // The set, not the photograph: with nothing else open
                    // there is nothing for this to paste onto.
                    .disabled(!(store?.hasGrade ?? false) || (store?.library.count ?? 0) < 2)
                    .help("The grade only — a crop belongs to the frame it was drawn on")
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
        // More than one, because a set is the thing the filmstrip, Export All
        // and Paste to All are all about — and picking two photographs in the
        // panel is how somebody makes one without reaching for a whole folder.
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK, !panel.urls.isEmpty else { return }
        store?.openPaths(panel.urls)
    }

    /// A folder of photographs, which is what a set usually is.
    ///
    /// `main.rs`'s Ctrl+Shift+O. Without it the Mac could only ever open one
    /// photograph at a time, which left the filmstrip with nothing to show,
    /// Export All with nothing to run on and Paste to All permanently greyed —
    /// three finished features that could not be reached.
    ///
    /// The scanning is the engine's, not this panel's: which extensions count
    /// and what order they come back in is one answer, kept beside the library
    /// that holds them.
    private func openFolder() {
        let panel = NSOpenPanel()
        panel.title = "Open folder"
        panel.prompt = "Open"
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        // Where the photograph in hand came from is where somebody starts
        // looking for the folder they mean.
        panel.directoryURL = store?.snapshot.path.map {
            URL(fileURLWithPath: $0).deletingLastPathComponent()
        }
        guard panel.runModal() == .OK, let url = panel.url else { return }
        store?.openFolder(url)
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
