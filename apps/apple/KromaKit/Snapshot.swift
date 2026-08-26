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
    /// The crop, straighten, quarter-turns and flips.
    ///
    /// Not optional, and not part of the stack: geometry sits *before* every
    /// row, so with nothing open this is the identity rather than nothing at
    /// all. See `GeometryValue`.
    public let geometry: GeometryValue
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
        case version, path, name, width, height, rows, geometry, passes
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
        rows: [], colour: Colour(input: "", output: ""), geometry: .identity,
        passes: 0,
        canUndo: false, canRedo: false, undoLabel: nil, redoLabel: nil,
        exportFormat: "jpeg", exportQuality: 95
    )

    public init(
        version: UInt64, isOpen: Bool, path: String?, name: String?,
        width: UInt32, height: UInt32, rows: [Row], colour: Colour,
        geometry: GeometryValue, passes: Int,
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
        self.geometry = geometry
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

/// The crop, straighten, quarter-turns and flips — `pe_core::Geometry`.
///
/// Seven fields and no arithmetic. Every rule about what makes one *legal* —
/// that the crop stays inside the straightened source, that a locked aspect is
/// honoured, that dragging a corner past the frame slides the rectangle rather
/// than shrinking it — lives on `Geometry` in Rust, where it is tested. This
/// side proposes a value and `Session.setGeometry` hands back the one the
/// engine actually stored. Nothing here reimplements `shrink_to_fit`,
/// `slide_to_fit` or `apply_aspect`, and nothing here should.
///
/// `CGPoint` and `CGSize` rather than pairs of `Float`, for the same reason
/// `CurveValue` carries points: everything that draws a crop wants a rectangle,
/// and the conversion would otherwise happen at every call site.
public struct GeometryValue: Decodable, Sendable, Equatable {
    /// Centre of the crop as an offset from the middle of the source, in units
    /// of the source's own width and height. Zero is dead centre.
    public let centre: CGPoint
    /// Size of the crop as a fraction of the source.
    public let size: CGSize
    /// Straightening angle in degrees. Positive turns the picture
    /// anticlockwise, which is the direction Lightroom's Angle slider moves.
    public let angle: Double
    /// Quarter-turns clockwise, applied after straightening. The engine stores
    /// 0 to 3 and takes whatever it is given modulo four.
    public let turns: Int
    public let flipH: Bool
    public let flipV: Bool
    public let aspect: AspectLock

    /// The whole frame, unturned and unflipped — `Geometry::default()`, and
    /// what the snapshot carries with nothing open.
    public static let identity = GeometryValue(
        centre: .zero, size: CGSize(width: 1, height: 1), angle: 0, turns: 0,
        flipH: false, flipV: false, aspect: .free
    )

    enum CodingKeys: String, CodingKey {
        case centre, size, angle, turns, aspect
        case flipH = "flip_h"
        case flipV = "flip_v"
        case aspectW = "aspect_w"
        case aspectH = "aspect_h"
    }

    public init(
        centre: CGPoint, size: CGSize, angle: Double, turns: Int,
        flipH: Bool, flipV: Bool, aspect: AspectLock
    ) {
        self.centre = centre
        self.size = size
        self.angle = angle
        self.turns = turns
        self.flipH = flipH
        self.flipV = flipV
        self.aspect = aspect
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let centre = try c.decode([Double].self, forKey: .centre)
        let size = try c.decode([Double].self, forKey: .size)
        self.centre = CGPoint(x: centre.first ?? 0, y: centre.dropFirst().first ?? 0)
        self.size = CGSize(width: size.first ?? 0, height: size.dropFirst().first ?? 0)
        angle = try c.decode(Double.self, forKey: .angle)
        turns = try c.decode(Int.self, forKey: .turns)
        flipH = try c.decode(Bool.self, forKey: .flipH)
        flipV = try c.decode(Bool.self, forKey: .flipV)

        // The lock is the one value here spread across three keys: a string
        // naming the arm, and two numbers that only a ratio has.
        switch try c.decode(String.self, forKey: .aspect) {
        case "original":
            aspect = .original
        case "ratio":
            // Both numbers or neither. A ratio with nothing to hold is not a
            // ratio, and reading it as one would divide by a zero the document
            // never wrote.
            if let w = try c.decodeIfPresent(Double.self, forKey: .aspectW),
                let h = try c.decodeIfPresent(Double.self, forKey: .aspectH) {
                aspect = .ratio(w: w, h: h)
            } else {
                aspect = .free
            }
        default:
            // Free, and anything a later version grows that this build has
            // never heard of — the same reason `ParamValue` has `.opaque`. A
            // lock nobody recognises is one nobody is holding.
            aspect = .free
        }
    }

    /// True when this does nothing at all, so the viewer can take the plain
    /// path and the panel can say "Original". Matching `Geometry::is_identity`,
    /// including what it leaves out: the lock is not part of it, because a lock
    /// constrains the next drag rather than changing the picture.
    ///
    /// Exactly, not nearly. A crop dragged and put back reads as untouched
    /// because the engine writes the value it computed, not an accumulated
    /// delta.
    public var isIdentity: Bool {
        centre == .zero && size == CGSize(width: 1, height: 1)
            && angle == 0 && turns % 4 == 0 && !flipH && !flipV
    }
}

/// What the crop's proportions are pinned to while the user drags it —
/// `pe_core::AspectLock`.
///
/// Three arms, one of them with a payload. It reaches Swift two different ways
/// and neither is this shape: the snapshot spells it as a string plus two
/// numbers, and the drag path spells it as a single float. `GeometryValue`'s
/// decoder does the first; `Session` does the second, because that spelling is
/// the C ABI's and only `Engine.swift` may touch that.
public enum AspectLock: Sendable, Equatable {
    case free
    /// The source photograph's own proportions.
    case original
    /// A fixed ratio, width to height.
    case ratio(w: Double, h: Double)

    /// The ratio this holds, width over height — 16:9 as 1.777…. Nil when
    /// there is no fixed one: Free has none, and Original's depends on the
    /// source, which is the engine's business to know and not this side's to
    /// work out.
    ///
    /// The guard on the divisor is `AspectLock::ratio`'s, so a malformed lock
    /// from a document on disk answers with a finite number rather than an
    /// infinity.
    public var widthOverHeight: Double? {
        if case let .ratio(w, h) = self { return w / max(h, 1e-6) }
        return nil
    }
}

/// One parameter's value, in the document's own representation.
///
/// Adjacently tagged as `{"t": "float", "v": 0.35}`, which is what
/// `pe-core`'s `ParamValue` writes. Every kind the engine writes today decodes
/// as itself; a kind this build has never heard of decodes as `.opaque` rather
/// than failing, so a photograph written by a later version still opens.
public enum ParamValue: Decodable, Sendable, Equatable {
    case float(Float)
    case bool(Bool)
    case choice(String)
    case rgb([Float])
    case wheel(WheelValue)
    case curve(CurveValue)
    case warp(WarpValue)
    case pins([PinValue])
    /// A kind this build does not know. Carries its tag so the inspector can
    /// say what it is declining to show.
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
        case "warp":
            self = .warp(try c.decode(WarpValue.self, forKey: .v))
        case "pins":
            // A bare array, like a curve's: `Pins` is transparent over its
            // `Vec`, so there is no object around it.
            self = .pins(try c.decode([PinValue].self, forKey: .v))
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

    /// The value as a lattice, for the warper that draws one.
    public var warpValue: WarpValue? {
        if case let .warp(w) = self { return w }
        return nil
    }

    /// The value as a set of pins, for the diagram that draws them.
    public var pinsValue: [PinValue]? {
        if case let .pins(p) = self { return p }
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

/// A lattice of displacements — one of the Colour Warper's grids.
///
/// What is stored is the *displacement* at each vertex, not the position.
/// An untouched lattice is all zeros, so it is obviously identity and costs
/// nothing to compare.
public struct WarpValue: Decodable, Sendable, Equatable {
    public let cols: Int
    public let rows: Int
    /// Row-major, `cols * rows` of them.
    public let offsets: [CGPoint]

    enum CodingKeys: String, CodingKey {
        case cols, rows, offsets
    }

    public init(cols: Int, rows: Int, offsets: [CGPoint]) {
        self.cols = cols
        self.rows = rows
        self.offsets = offsets
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        cols = try c.decode(Int.self, forKey: .cols)
        rows = try c.decode(Int.self, forKey: .rows)
        let pairs = try c.decode([[Double]].self, forKey: .offsets)
        offsets = pairs.map { CGPoint(x: $0.first ?? 0, y: $0.dropFirst().first ?? 0) }
    }

    /// The displacement at a vertex, or none for a vertex the grid does not
    /// have. Matching `Warp::at`, which returns zero rather than trapping.
    public func at(col: Int, row: Int) -> CGPoint {
        guard col >= 0, row >= 0, col < cols, row < rows else { return .zero }
        let i = row * cols + col
        return offsets.indices.contains(i) ? offsets[i] : .zero
    }

    /// A lattice with one vertex moved, for a view holding an in-flight drag.
    public func replacing(col: Int, row: Int, with offset: CGPoint) -> WarpValue {
        guard col >= 0, row >= 0, col < cols, row < rows else { return self }
        let i = row * cols + col
        guard offsets.indices.contains(i) else { return self }
        var next = offsets
        next[i] = offset
        return WarpValue(cols: cols, rows: rows, offsets: next)
    }

    /// Whether this lattice leaves the picture alone. Exactly zero rather than
    /// nearly: a vertex dragged and put back should read as untouched, and
    /// drags land on exact values because the widget writes the position it
    /// computed, not an accumulated delta.
    public var isIdentity: Bool {
        offsets.allSatisfy { $0 == .zero }
    }
}

/// One pin on the chromaticity diagram.
///
/// `at` is where the colour is and `to` is where it should go — both **CIE xy
/// chromaticities**, not fractions of the plot. `PinGeometry` is what turns one
/// into a position.
///
/// The wire shape is an object with snake_case keys, inside a bare array —
/// `pe_core::Pins` is `#[serde(transparent)]` over its `Vec`, so only the pin
/// itself has field names.
public struct PinValue: Decodable, Sendable, Equatable {
    public let at: CGPoint
    public let to: CGPoint
    /// How far around `at` the pull reaches, in the same units.
    public let chromaRange: Double
    /// How much of the pull the shadows and highlights take, and where the
    /// boundary sits. Both at one is every tone equally, which is why both
    /// default to one.
    public let tonalLow: Double
    public let tonalHigh: Double
    public let tonalPivot: Double
    /// Stops of light, applied within the pin's reach.
    public let exposure: Double

    /// How many pins one warper may carry, matching `pe_core::pins::MAX_PINS`.
    /// Bounded because they travel to the GPU inside the curve LUT, and because
    /// the honest number is small.
    public static let maxPins = 8

    enum CodingKeys: String, CodingKey {
        case at, to, exposure
        case chromaRange = "chroma_range"
        case tonalLow = "tonal_low"
        case tonalHigh = "tonal_high"
        case tonalPivot = "tonal_pivot"
    }

    public init(
        at: CGPoint, to: CGPoint, chromaRange: Double,
        tonalLow: Double, tonalHigh: Double, tonalPivot: Double, exposure: Double
    ) {
        self.at = at
        self.to = to
        self.chromaRange = chromaRange
        self.tonalLow = tonalLow
        self.tonalHigh = tonalHigh
        self.tonalPivot = tonalPivot
        self.exposure = exposure
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let a = try c.decode([Double].self, forKey: .at)
        let t = try c.decode([Double].self, forKey: .to)
        at = CGPoint(x: a.first ?? 0, y: a.dropFirst().first ?? 0)
        to = CGPoint(x: t.first ?? 0, y: t.dropFirst().first ?? 0)
        chromaRange = try c.decode(Double.self, forKey: .chromaRange)
        tonalLow = try c.decode(Double.self, forKey: .tonalLow)
        tonalHigh = try c.decode(Double.self, forKey: .tonalHigh)
        tonalPivot = try c.decode(Double.self, forKey: .tonalPivot)
        exposure = try c.decode(Double.self, forKey: .exposure)
    }

    /// A pin placed at a point and not yet moved. Matching `Pin::placed`.
    public static func placed(at: CGPoint) -> PinValue {
        PinValue(at: at, to: at, chromaRange: 0.04,
                 tonalLow: 1, tonalHigh: 1, tonalPivot: 0.5, exposure: 0)
    }

    /// Whether this pin leaves the picture alone.
    ///
    /// A pin placed but not dragged is not a no-op waiting to happen — it is
    /// one the user put somewhere deliberately and is about to move. Exposure
    /// counts too: a pin can be dead centre and still be brightening the
    /// picture.
    public var isNeutral: Bool {
        at == to && exposure == 0
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
