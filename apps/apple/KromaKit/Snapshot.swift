import Foundation

/// Everything a shell needs to draw itself, in one document.
///
/// Derived from the engine, never authored. Mutations go one way in through
/// typed calls; this comes one way back out. Two directions and one source of
/// truth, which is what keeps an `@Observable` view model idiomatic without it
/// becoming a second implementation of the document.
public struct Snapshot: Decodable, Sendable {
    /// Bumped by every mutation. Compare before decoding anything.
    public let version: UInt64
    public let isOpen: Bool
    public let path: String?
    /// File name alone, for the title bar.
    public let name: String?
    public let width: UInt32
    public let height: UInt32
    public let rows: [Row]
    public let colour: Colour
    /// Passes the last frame executed. The number that proves the stage cache
    /// works: with a deep stack, dragging one slider should read 1.
    public let passes: Int
    public let canUndo: Bool
    public let canRedo: Bool
    public let undoLabel: String?
    public let redoLabel: String?
    public let exportFormat: String
    public let exportQuality: UInt8

    enum CodingKeys: String, CodingKey {
        case version, path, name, width, height, rows, passes
        case isOpen = "is_open"
        case colour = "color"
        case canUndo = "can_undo"
        case canRedo = "can_redo"
        case undoLabel = "undo_label"
        case redoLabel = "redo_label"
        case exportFormat = "export_format"
        case exportQuality = "export_quality"
    }

    public static let empty = Snapshot(
        version: 0, isOpen: false, path: nil, name: nil, width: 0, height: 0,
        rows: [], colour: Colour(input: "", output: ""), passes: 0,
        canUndo: false, canRedo: false, undoLabel: nil, redoLabel: nil,
        exportFormat: "jpeg", exportQuality: 95
    )

    public init(
        version: UInt64, isOpen: Bool, path: String?, name: String?,
        width: UInt32, height: UInt32, rows: [Row], colour: Colour, passes: Int,
        canUndo: Bool, canRedo: Bool, undoLabel: String?, redoLabel: String?,
        exportFormat: String, exportQuality: UInt8
    ) {
        self.version = version
        self.isOpen = isOpen
        self.path = path
        self.name = name
        self.width = width
        self.height = height
        self.rows = rows
        self.colour = colour
        self.passes = passes
        self.canUndo = canUndo
        self.canRedo = canRedo
        self.undoLabel = undoLabel
        self.redoLabel = redoLabel
        self.exportFormat = exportFormat
        self.exportQuality = exportQuality
    }

    public struct Colour: Decodable, Sendable {
        public let input: String
        public let output: String

        public init(input: String, output: String) {
            self.input = input
            self.output = output
        }
    }

    public struct Row: Decodable, Sendable, Identifiable {
        public let id: UInt64
        public let effect: String
        public let enabled: Bool
        public let opacity: Float
        public let blend: String
        /// Fixed panels, which cannot be removed or reordered.
        public let pinned: Bool
        public let label: String?
        public let params: [String: ParamValue]
    }
}

/// One parameter's value, in the document's own representation.
///
/// Adjacently tagged as `{"t": "float", "v": 0.35}`, which is what
/// `pe-core`'s `ParamValue` writes. Kinds this slice does not draw — warps,
/// pin lattices — decode as `.opaque` rather than failing, so a photograph
/// carrying one still opens.
public enum ParamValue: Decodable, Sendable, Equatable {
    case float(Float)
    case bool(Bool)
    case choice(String)
    case rgb([Float])
    case wheel(WheelValue)
    case curve(CurveValue)
    /// Structure this build does not draw. Carries its tag so the inspector
    /// can say what it is declining to show.
    case opaque(String)

    enum CodingKeys: String, CodingKey {
        case t, v
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let tag = try c.decode(String.self, forKey: .t)
        switch tag {
        case "float":
            self = .float(try c.decode(Float.self, forKey: .v))
        case "int":
            self = .float(Float(try c.decode(Int.self, forKey: .v)))
        case "bool":
            self = .bool(try c.decode(Bool.self, forKey: .v))
        case "choice":
            self = .choice(try c.decode(String.self, forKey: .v))
        case "rgb":
            self = .rgb(try c.decode([Float].self, forKey: .v))
        case "wheel":
            self = .wheel(try c.decode(WheelValue.self, forKey: .v))
        case "curve":
            self = .curve(try c.decode(CurveValue.self, forKey: .v))
        default:
            self = .opaque(tag)
        }
    }

    /// The value as a number, for anything that draws one. Nil when it is not
    /// a number at all.
    public var floatValue: Float? {
        if case let .float(v) = self { return v }
        return nil
    }

    /// The value as a wheel, for the control that draws one.
    public var wheelValue: WheelValue? {
        if case let .wheel(w) = self { return w }
        return nil
    }

    /// The value as a curve, for the editor that draws one.
    public var curveValue: CurveValue? {
        if case let .curve(c) = self { return c }
        return nil
    }
}

/// A curve's control points.
///
/// `CGPoint` rather than pairs of `Float`, because everything that draws one
/// wants points and the conversion would otherwise happen at every call site.
///
/// The wire shape is a bare array of pairs, not an object — `pe_core::Curve`
/// is `#[serde(transparent)]`, so the struct's field name never reaches the
/// JSON.
public struct CurveValue: Decodable, Sendable, Equatable {
    public let points: [CGPoint]

    public init(points: [CGPoint]) {
        self.points = points
    }

    public init(from decoder: Decoder) throws {
        let pairs = try [[Double]](from: decoder)
        points = pairs.map { CGPoint(x: $0.first ?? 0, y: $0.dropFirst().first ?? 0) }
    }
}

/// A four-way colour wheel's value.
///
/// Three channels and a master. `pe-core` keeps the master separate rather than
/// folding it into the channels so that resetting only the outer ring stays
/// possible, and the wire shape follows.
public struct WheelValue: Decodable, Sendable, Equatable {
    public let rgb: [Float]
    public let master: Float

    public init(rgb: [Float], master: Float) {
        self.rgb = rgb
        self.master = master
    }
}
