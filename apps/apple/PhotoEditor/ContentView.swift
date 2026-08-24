import SwiftUI

struct ContentView: View {
    var body: some View {
        Text("engine \(Engine.version)")
            .monospaced()
            .padding(32)
            .frame(minWidth: 480, minHeight: 320)
    }
}
