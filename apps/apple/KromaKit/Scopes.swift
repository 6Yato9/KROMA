import Foundation

/// One frame's measurements, copied out of the engine.
///
/// The engine counts; this holds the counts and nothing else. Turning them
/// into a picture — the brightness curve, the graticule, the colours — belongs
/// to the views, so the numbers stay testable without a display. That is the
/// same division `pe-scopes` states in its own header, and it is what lets
/// these cross a C ABI as plain buffers.
///
/// Everything here is nested rather than top-level for a reason that is not
/// taste. `cbindgen.toml` exports with an empty prefix, so `PeScope`'s
/// enumerators land in C's global namespace and Swift imports them as bare
/// `Histogram`, `ColourSpread`, `Waveform`, `Vectorscope` and friends. A Swift
/// type with one of those names would be ambiguous with the constant at every
/// call site; nesting keeps the names this file wants without spelling every
/// other enum in the header differently.
public struct Scopes: Sendable {
    public let histogram: Levels
    /// The same frame binned in the curve's own domain, for drawing behind the
    /// curve editor.
    public let logHistogram: Levels
    /// Where the frame's hues and saturations sit, for the secondary curves.
    public let colour: Spread
    public let waveform: WaveformCounts
    public let vectorscope: Plane
    /// Where the frame's colours sit on each of the Colour Warper's plots.
    public let warper: WarperClouds
    /// Which measurement this is. Strictly increasing, so a holder of a copy
    /// can tell whether the engine's numbers have moved without asking for
    /// them — which is the whole reason a 2.6 MB waveform is affordable.
    public let generation: UInt64
}

/// What a frame is measured at.
///
/// The scope's size, not the photograph's: a waveform has one column per pixel
/// of the measured frame, so this is how wide the panel that will draw it is.
/// Nested nowhere, because it is what a view hands the store rather than part
/// of what comes back.
public struct ScopeSize: Equatable, Sendable {
    public let width: UInt32
    public let height: UInt32

    public init(width: UInt32, height: UInt32) {
        self.width = width
        self.height = height
    }
}

extension Scopes {
    /// Which plane of a four-channel measurement. The order is the ABI's.
    public enum Channel: Int, Sendable, CaseIterable {
        case red = 0
        case green = 1
        case blue = 2
        case luma = 3
    }

    /// Four channels binned into levels: the histogram, and the histogram in
    /// the curve's own domain.
    public struct Levels: Sendable {
        public let red: [UInt32]
        public let green: [UInt32]
        public let blue: [UInt32]
        public let luma: [UInt32]
        /// Pixels measured. Each channel's counts sum to this.
        public let total: UInt32
        /// The largest count in any channel.
        public let peak: UInt32

        public var bins: Int { luma.count }

        /// What a count is drawn against.
        ///
        /// The tallest bin, because a histogram's shape is the whole point and
        /// a frame with one dominant level would otherwise draw as a single
        /// spike over a flat floor.
        public var fullScale: UInt32 { peak }

        /// One channel's counts. No copy: `[UInt32]` is copy-on-write, so this
        /// hands out a reference until somebody writes to it, and nobody does.
        public func plane(_ channel: Channel) -> [UInt32] {
            switch channel {
            case .red: return red
            case .green: return green
            case .blue: return blue
            case .luma: return luma
            }
        }

        /// One bin as a fraction of full scale, ready to be a bar's height.
        /// The arithmetic lives here so no view has to remember which of
        /// `total` and `peak` this scope is read against.
        public func fraction(_ channel: Channel, bin: Int) -> Double {
            guard fullScale > 0 else { return 0 }
            return Double(plane(channel)[bin]) / Double(fullScale)
        }
    }

    /// Hue and saturation spread, for behind the secondary curves.
    public struct Spread: Sendable {
        public let hue: [UInt32]
        public let saturation: [UInt32]
        /// Pixels measured.
        public let total: UInt32
        /// The largest count in either plane.
        public let peak: UInt32

        public var bins: Int { hue.count }
        public var fullScale: UInt32 { peak }

        public func fraction(hueBin bin: Int) -> Double {
            guard fullScale > 0 else { return 0 }
            return Double(hue[bin]) / Double(fullScale)
        }

        public func fraction(saturationBin bin: Int) -> Double {
            guard fullScale > 0 else { return 0 }
            return Double(saturation[bin]) / Double(fullScale)
        }
    }

    /// One row of levels per image column, per channel: the waveform, and the
    /// parade, which is the same counts drawn three times side by side.
    public struct WaveformCounts: Sendable {
        /// Columns of the frame, which is the width the measurement was asked
        /// for rather than the width of the photograph.
        public let columns: Int
        /// Levels per column. 256.
        public let levels: Int
        /// Image rows that fed each column.
        public let total: UInt32

        private let red: [UInt32]
        private let green: [UInt32]
        private let blue: [UInt32]
        private let luma: [UInt32]

        init(
            columns: Int, levels: Int, total: UInt32,
            red: [UInt32], green: [UInt32], blue: [UInt32], luma: [UInt32]
        ) {
            self.columns = columns
            self.levels = levels
            self.total = total
            self.red = red
            self.green = green
            self.blue = blue
            self.luma = luma
        }

        /// What a cell is drawn against: the number of rows that fed the
        /// column, not the brightest cell. Unlike a peak it does not move as
        /// the picture is graded, so the display does not flicker under the
        /// user's hand. It is also why this scope never asks the engine for a
        /// peak, which is the one field that costs a walk over the counts.
        public var fullScale: UInt32 { total }

        /// One channel's counts, row-major, `columns` rows of `levels`.
        /// No copy; see `Levels.plane`.
        public func plane(_ channel: Channel) -> [UInt32] {
            switch channel {
            case .red: return red
            case .green: return green
            case .blue: return blue
            case .luma: return luma
            }
        }

        public func at(_ channel: Channel, column: Int, level: Int) -> UInt32 {
            plane(channel)[column * levels + level]
        }

        /// One cell as a fraction of full scale, ready to be an opacity.
        public func fraction(_ channel: Channel, column: Int, level: Int) -> Double {
            guard fullScale > 0 else { return 0 }
            return Double(at(channel, column: column, level: level)) / Double(fullScale)
        }
    }

    /// A single grid of counts: the vectorscope, and each of the warper's
    /// three clouds.
    public struct Plane: Sendable {
        /// Row-major, `height` rows of `width`.
        public let counts: [UInt32]
        public let width: Int
        public let height: Int
        /// Pixels measured. Not the sum of the counts for a warper cloud,
        /// where black has no chromaticity and is never binned.
        public let total: UInt32
        /// The largest count anywhere in the grid.
        public let peak: UInt32

        /// What a cell is drawn against. The peak: a cloud's shape is what is
        /// being read off it, and against the pixel count almost every cell
        /// would round to nothing.
        public var fullScale: UInt32 { peak }

        public func at(x: Int, y: Int) -> UInt32 {
            counts[y * width + x]
        }

        /// One cell as a fraction of full scale, ready to be an opacity.
        public func fraction(x: Int, y: Int) -> Double {
            guard fullScale > 0 else { return 0 }
            return Double(at(x: x, y: y)) / Double(fullScale)
        }
    }

    /// The Colour Warper's three clouds, one per plot.
    public struct WarperClouds: Sendable {
        public let chromaticity: Plane
        public let hueSat: Plane
        public let chromaLuma: Plane
    }
}
