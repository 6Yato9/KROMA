import SwiftUI

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
    }
}
