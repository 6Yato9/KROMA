import CoreTransferable
import UniformTypeIdentifiers

/// An effect being dragged out of the shelf.
///
/// Its own uniform type rather than a bare `String`, and that is the whole
/// point of the type existing: a drop must add an effect only when the thing
/// dropped came from this application's shelf. Text dragged in from a browser
/// or an editor is not an effect and must not become one — and a `String`
/// payload would make every one of them a valid drop.
///
/// The key is carried, not the whole `Effect`. The registry is the authority on
/// what an effect is, and shipping a copy of one through a pasteboard would be
/// a second copy that could disagree with it.
public struct DraggedEffect: Codable, Sendable, Transferable, Equatable {
    public let key: String

    public init(key: String) {
        self.key = key
    }

    /// Declared in the application's Info.plist as an exported type. An
    /// undeclared identifier works inside one process and fails between two,
    /// which is exactly the case a drag is.
    public static let type = UTType(exportedAs: "com.kroma.effect-key")

    public static var transferRepresentation: some TransferRepresentation {
        CodableRepresentation(contentType: type)
    }

    // ---- what the representation does, reachable from a test ---------------

    /// `CodableRepresentation` is JSON over the declared type. Spelled out here
    /// so the round trip can be asserted without driving a pasteboard: the
    /// conformance above is a thin wrapper over these two.
    public func encoded() throws -> Data {
        try JSONEncoder().encode(self)
    }

    public static func decoded(from data: Data) throws -> DraggedEffect {
        try JSONDecoder().decode(DraggedEffect.self, from: data)
    }
}
