import SwiftUI

/// M0 placeholder.
///
/// Its only job is to prove the Swift side links against the Rust engine and
/// can round-trip a document across the C ABI. The real Colour Page — viewer,
/// scopes, palette strip, inspector — arrives with the Mac port at M6, by which
/// point the engine below it is finished and unchanged.
struct ContentView: View {
    @State private var status: String = "not run"

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Photo Editor")
                .font(.largeTitle.weight(.semibold))
            Text("engine \(Engine.version)")
                .foregroundStyle(.secondary)
                .monospaced()

            Divider()

            Text(status)
                .monospaced()
                .textSelection(.enabled)

            Button("Round-trip a document through the engine") {
                status = roundTrip()
            }
        }
        .padding(32)
        .frame(minWidth: 480, minHeight: 320)
    }

    private func roundTrip() -> String {
        let json = """
        {
          "schema_version": 1,
          "source": {"kind": "path", "path": "DSCF1234.JPG"},
          "stack": [
            {"id": 1, "effect": "exposure", "params": {"ev": {"t": "float", "v": 0.35}}},
            {"id": 2, "effect": "halation", "opacity": 0.4, "blend": "screen"}
          ]
        }
        """
        guard let doc = Document(json: json) else {
            return "engine rejected the document"
        }
        guard let back = doc.toJSON() else {
            return "engine could not serialise the document"
        }
        return "ok — \(doc.rowCount) rows, \(back.count) bytes back"
    }
}
