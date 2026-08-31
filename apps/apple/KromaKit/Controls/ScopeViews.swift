import CoreGraphics
import Foundation
import SwiftUI

// -----------------------------------------------------------------------------
// Turning counts into pixels
// -----------------------------------------------------------------------------

/// The tint one channel is drawn in, as three unit components.
///
/// Additive, so where all three overlap the result goes pale — the same
/// convention as the histogram, and the only one that does not hide whichever
/// channel happens to be drawn last.
public struct ScopeTint: Sendable, Equatable {
    public let red: Double
    public let green: Double
    public let blue: Double

    public init(_ red: Double, _ green: Double, _ blue: Double) {
        self.red = red
        self.green = green
        self.blue = blue
    }
}

/// How a grid of counts becomes an image.
///
/// A waveform is 640 columns by 256 levels. As SwiftUI geometry that is a
/// hundred and sixty thousand rectangles a frame; as a `CGImage` built once per
/// measurement it is one draw. That is the same reason the Windows shell
/// uploads a texture rather than emitting quads, and it is why everything in
/// here produces bytes rather than shapes.
///
/// All of it is static and takes only counts, so the arithmetic that decides
/// what a scope *says* is testable without a display.
public enum ScopeImage {

    // ---- brightness ------------------------------------------------------

    /// How a cell's count becomes a brightness.
    ///
    /// `fullScale` is the number of image rows that fed the column, not the
    /// brightest cell anywhere. A cell's count is bounded by the former, so it
    /// is the natural ceiling — and unlike a peak it does not move as the
    /// picture is graded, so the display does not flicker under the user's
    /// hand.
    ///
    /// The square root is the part that makes it readable. A flat sky puts a
    /// whole column in one cell and a gradient spreads it over two hundred; on
    /// a linear scale the gradient is one two-hundredth as bright as the sky,
    /// which is to say invisible. Every hardware scope applies a curve here for
    /// exactly this reason.
    public static func intensity(count: UInt32, fullScale: Int) -> Double {
        guard count > 0 else { return 0 }
        let fraction = Double(count) / Double(max(fullScale, 1))
        return min(max(fraction.squareRoot() * 1.7, 0.06), 1)
    }

    /// The same job for a cloud with no natural ceiling.
    ///
    /// A vectorscope has none: a flat frame lands entirely in one cell and a
    /// rainbow spreads over thousands, so the only thing to read against is the
    /// observed peak. The flicker `intensity` avoids is the price, and it buys
    /// a scope that is not blank.
    public static func cloud(count: UInt32, peak: Int) -> Double {
        guard count > 0 else { return 0 }
        let fraction = Double(count) / Double(max(peak, 1))
        return min(max(pow(fraction, 0.4) * 1.1, 0.1), 1)
    }

    /// The tint each channel is drawn in.
    public static func tint(_ channel: Scopes.Channel) -> ScopeTint {
        switch channel {
        case .red: return ScopeTint(1.0, 0.18, 0.18)
        case .green: return ScopeTint(0.25, 1.0, 0.35)
        case .blue: return ScopeTint(0.35, 0.5, 1.0)
        case .luma: return ScopeTint(0.82, 0.88, 0.95)
        }
    }

    /// The vectorscope's own tint. One colour, because the cloud's shape is
    /// what is read off it and there is nothing to tell apart.
    public static let cloudTint = ScopeTint(0.55, 1.0, 0.7)

    // ---- the bytes -------------------------------------------------------

    /// A block of RGBA8 a `CGImage` is made from.
    ///
    /// Alpha last and premultiplied, which for an additive plot means alpha is
    /// 255 wherever anything was counted and 0 everywhere else — so the well
    /// behind shows through the empty parts of the scope rather than being
    /// covered by black.
    public struct Raster: Sendable {
        public let width: Int
        public let height: Int
        public private(set) var bytes: [UInt8]

        init(width: Int, height: Int) {
            self.width = max(width, 1)
            self.height = max(height, 1)
            bytes = [UInt8](repeating: 0, count: self.width * self.height * 4)
        }

        /// Additive, saturating.
        mutating func add(x: Int, y: Int, tint: ScopeTint, amount: Double) {
            guard amount > 0, x >= 0, x < width, y >= 0, y < height else { return }
            let i = (y * width + x) * 4
            bytes[i] = Self.mix(bytes[i], tint.red, amount)
            bytes[i + 1] = Self.mix(bytes[i + 1], tint.green, amount)
            bytes[i + 2] = Self.mix(bytes[i + 2], tint.blue, amount)
            bytes[i + 3] = 255
        }

        private static func mix(_ base: UInt8, _ tint: Double, _ amount: Double) -> UInt8 {
            UInt8(min(Double(base) + tint * amount * 255, 255))
        }

        public func pixel(x: Int, y: Int) -> (r: UInt8, g: UInt8, b: UInt8, a: UInt8) {
            guard x >= 0, x < width, y >= 0, y < height else { return (0, 0, 0, 0) }
            let i = (y * width + x) * 4
            return (bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3])
        }

        public func alpha(x: Int, y: Int) -> UInt8 { pixel(x: x, y: y).a }

        public func cgImage() -> CGImage? {
            guard let provider = CGDataProvider(data: Data(bytes) as CFData) else { return nil }
            return CGImage(
                width: width,
                height: height,
                bitsPerComponent: 8,
                bitsPerPixel: 32,
                bytesPerRow: width * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                provider: provider,
                decode: nil,
                shouldInterpolate: false,
                intent: .defaultIntent
            )
        }
    }

    /// One panel, columns across and levels up.
    ///
    /// Several channels overlaid is the reading that shows a cast as the
    /// channels pulling apart at one end of the range; luma alone is what
    /// exposure is set against. Neither replaces the other.
    public static func waveform(
        _ counts: Scopes.WaveformCounts, channels: [Scopes.Channel]
    ) -> Raster {
        var raster = Raster(width: counts.columns, height: counts.levels)
        for channel in channels {
            paint(&raster, counts, channel, xOffset: 0)
        }
        return raster
    }

    /// Three panels side by side: the same counts as `waveform`, laid out
    /// rather than overlaid, which is the reading you want when you are chasing
    /// a cast rather than exposure.
    public static func parade(_ counts: Scopes.WaveformCounts) -> Raster {
        var raster = Raster(width: counts.columns * 3, height: counts.levels)
        for (panel, channel) in [Scopes.Channel.red, .green, .blue].enumerated() {
            paint(&raster, counts, channel, xOffset: panel * counts.columns)
        }
        return raster
    }

    private static func paint(
        _ raster: inout Raster,
        _ counts: Scopes.WaveformCounts,
        _ channel: Scopes.Channel,
        xOffset: Int
    ) {
        let levels = counts.levels
        let full = Int(counts.fullScale)
        let ink = tint(channel)
        let plane = counts.plane(channel)
        let top = levels - 1
        for column in 0..<counts.columns {
            let base = column * levels
            for level in 0..<levels {
                let count = plane[base + level]
                if count == 0 { continue }
                // Level 0 is black, and black belongs at the *bottom* of the
                // plot. Getting this upside down draws a plausible waveform of
                // a photograph nobody took.
                raster.add(
                    x: xOffset + column,
                    y: top - level,
                    tint: ink,
                    amount: intensity(count: count, fullScale: full)
                )
            }
        }
    }

    /// The vectorscope's square. The grid already runs y downwards for drawing,
    /// so the rows go straight across.
    public static func vectorscope(_ plane: Scopes.Plane) -> Raster {
        var raster = Raster(width: plane.width, height: plane.height)
        let peak = Int(plane.fullScale)
        let counts = plane.counts
        for y in 0..<plane.height {
            let base = y * plane.width
            for x in 0..<plane.width {
                let count = counts[base + x]
                if count == 0 { continue }
                raster.add(
                    x: x, y: y, tint: cloudTint, amount: cloud(count: count, peak: peak)
                )
            }
        }
        return raster
    }
}

// -----------------------------------------------------------------------------
// The graticule
// -----------------------------------------------------------------------------

/// What a vectorscope is read *against*: the six colour-bar boxes and the skin
/// line.
///
/// `pe_scopes::waveform::position`, `TARGETS` and `SKIN` are `pub` Rust with no
/// C ABI, so this is a second implementation of that projection — the same
/// choice the curve editor and the warper made, and kept honest the same way:
/// `apps/apple/Fixtures/scope_graticule.json` is written by
/// `cargo test -p pe-session --test fixtures`, carries the six positions *and*
/// the bin a pixel of each colour actually lands in when it goes through
/// `Vectorscope::from_display`, and `ScopeViewsTests` checks both. A box that
/// ended up somewhere the pixels cannot reach would fail there rather than
/// looking slightly wrong on screen, which is the whole property the boxes
/// have to have.
public enum ScopeGraticule {

    /// One labelled colour, as published.
    public struct Target: Sendable, Equatable {
        public let name: String
        public let red: UInt8
        public let green: UInt8
        public let blue: UInt8

        public init(name: String, red: UInt8, green: UInt8, blue: UInt8) {
            self.name = name
            self.red = red
            self.green = green
            self.blue = blue
        }
    }

    /// The six colour bar targets, at 75% — which is what the boxes on every
    /// hardware vectorscope mark, so a colourist reading ours can use what they
    /// already know. `pe_scopes::TARGETS`.
    public static let targets: [Target] = [
        Target(name: "R", red: 191, green: 0, blue: 0),
        Target(name: "Yl", red: 191, green: 191, blue: 0),
        Target(name: "G", red: 0, green: 191, blue: 0),
        Target(name: "Cy", red: 0, green: 191, blue: 191),
        Target(name: "B", red: 0, green: 0, blue: 191),
        Target(name: "Mg", red: 191, green: 0, blue: 191),
    ]

    /// A skin tone, for the line every vectorscope draws. One sample is enough:
    /// skin of every shade sits along the same hue axis and differs in how far
    /// out and how bright it is, not in which direction it points.
    /// `pe_scopes::SKIN`.
    public static let skin = Target(name: "skin", red: 198, green: 134, blue: 102)

    /// Where a display colour lands, in −1…1 with the centre at the origin and
    /// Cr pointing up. Rec.709 chroma, scaled so the primaries land inside the
    /// unit circle the way they do on a hardware scope.
    public static func position(red: UInt8, green: UInt8, blue: UInt8) -> CGPoint {
        let r = Double(red) / 255
        let g = Double(green) / 255
        let b = Double(blue) / 255
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b
        return CGPoint(x: (b - y) / 1.8556 * 2, y: (r - y) / 1.5748 * 2)
    }

    public static func position(_ target: Target) -> CGPoint {
        position(red: target.red, green: target.green, blue: target.blue)
    }

    /// The same position as a fraction of the plot, x rightwards and y
    /// **downwards** — the grid's own order, and the order a view draws in.
    public static func unit(_ p: CGPoint) -> CGPoint {
        CGPoint(x: (p.x + 1) * 0.5, y: (1 - p.y) * 0.5)
    }

    /// Which bin of a `size`-square vectorscope a position falls in.
    ///
    /// The same arithmetic and the same clamping `Vectorscope::from_display`
    /// does — clamp rather than drop, because a colour outside the plot is
    /// still a colour that is there.
    public static func cell(_ p: CGPoint, size: Int) -> (x: Int, y: Int) {
        let u = unit(p)
        return (
            min(max(Int(u.x * Double(size)), 0), size - 1),
            min(max(Int(u.y * Double(size)), 0), size - 1)
        )
    }
}

// -----------------------------------------------------------------------------
// The four views
// -----------------------------------------------------------------------------

/// The last image built, kept across body evaluations.
///
/// A reference type, written to from `body` — which is safe here precisely
/// because it is a memo and not state: nothing about what the view looks like
/// depends on whether the memo was hit or missed, so nothing needs
/// invalidating when it is written. Doing it this way rather than from
/// `onAppear` means the picture exists on the *first* body evaluation, in any
/// context that draws a view at all.
final class ScopeImageMemo {
    private var key: AnyHashable?
    private var image: CGImage?

    func image(for key: AnyHashable, build: () -> ScopeImage.Raster) -> CGImage? {
        guard self.key != key || image == nil else { return image }
        self.key = key
        image = build().cgImage()
        return image
    }
}

/// An image built from the counts, rebuilt only when `key` changes.
///
/// The key is what stops half a megapixel being rebuilt on every body
/// evaluation — which, with a slider under the user's thumb, is sixty times a
/// second. `Scopes.generation` is the natural key, plus whatever else changes
/// the picture without changing the measurement.
struct ScopePicture<Key: Hashable>: View {
    let key: Key
    let build: () -> ScopeImage.Raster

    @State private var memo = ScopeImageMemo()

    var body: some View {
        if let image = memo.image(for: key, build: build) {
            Image(decorative: image, scale: 1)
                .resizable()
                .interpolation(.low)
        } else {
            Color.clear
        }
    }
}

/// The horizontal reference lines a waveform is read against: black, the
/// quarters, and white. Without them a trace is a shape rather than a
/// measurement.
struct ScopeLevels: View {
    var body: some View {
        GeometryReader { geo in
            ZStack {
                line(geo.size, at: [0, 1]).stroke(.white.opacity(0.24), lineWidth: 1)
                line(geo.size, at: [0.25, 0.5, 0.75]).stroke(.white.opacity(0.1), lineWidth: 1)
            }
        }
        .allowsHitTesting(false)
    }

    private func line(_ size: CGSize, at levels: [CGFloat]) -> Path {
        Path { p in
            for level in levels {
                // Level runs up, so a fraction of 1 is the top of the box.
                let y = size.height * (1 - level)
                p.move(to: CGPoint(x: 0, y: y))
                p.addLine(to: CGPoint(x: size.width, y: y))
            }
        }
    }
}

/// One panel: columns across, levels up.
public struct WaveformView: View {
    private struct Key: Hashable {
        let generation: UInt64
        let channels: [Int]
    }

    let scopes: Scopes
    let channels: [Scopes.Channel]

    public init(scopes: Scopes, channels: [Scopes.Channel]) {
        self.scopes = scopes
        self.channels = channels
    }

    public var body: some View {
        let counts = scopes.waveform
        let picked = channels
        ZStack {
            ScopePicture(
                key: Key(generation: scopes.generation, channels: picked.map(\.rawValue))
            ) {
                ScopeImage.waveform(counts, channels: picked)
            }
            ScopeLevels()
        }
    }
}

/// Three panels side by side. The same counts as the waveform; what changes is
/// the reading.
public struct ParadeView: View {
    let scopes: Scopes

    public init(scopes: Scopes) {
        self.scopes = scopes
    }

    public var body: some View {
        let counts = scopes.waveform
        ZStack {
            ScopePicture(key: scopes.generation) {
                ScopeImage.parade(counts)
            }
            ScopeLevels()
            seams
        }
    }

    /// The two seams, so it reads as three scopes rather than one wide one.
    private var seams: some View {
        GeometryReader { geo in
            Path { p in
                for i in 1..<3 {
                    let x = geo.size.width * CGFloat(i) / 3
                    p.move(to: CGPoint(x: x, y: 0))
                    p.addLine(to: CGPoint(x: x, y: geo.size.height))
                }
            }
            .stroke(.white.opacity(0.16), lineWidth: 1)
        }
        .allowsHitTesting(false)
    }
}

/// The square, plus the graticule.
public struct VectorscopeView: View {
    let scopes: Scopes

    public init(scopes: Scopes) {
        self.scopes = scopes
    }

    public var body: some View {
        let plane = scopes.vectorscope
        ZStack {
            ScopePicture(key: scopes.generation) {
                ScopeImage.vectorscope(plane)
            }
            VectorscopeGraticule()
        }
        // Square, or the hue circle would be an ellipse and the boxes would
        // stop meaning anything.
        .aspectRatio(1, contentMode: .fit)
    }
}

/// The colour-bar boxes, the skin line and the rings, drawn by running the same
/// projection the pixels went through.
struct VectorscopeGraticule: View {
    var body: some View {
        Canvas { context, size in
            let side = min(size.width, size.height)
            let centre = CGPoint(x: size.width / 2, y: size.height / 2)
            let radius = side / 2
            rings(&context, centre: centre, radius: radius)
            skin(&context, centre: centre, radius: radius)
            boxes(&context, centre: centre, radius: radius)
        }
        .allowsHitTesting(false)
    }

    private func place(_ p: CGPoint, centre: CGPoint, radius: CGFloat) -> CGPoint {
        CGPoint(x: centre.x + p.x * radius, y: centre.y - p.y * radius)
    }

    private func rings(_ context: inout GraphicsContext, centre: CGPoint, radius: CGFloat) {
        let r = radius * 0.75
        let circle = Path(
            ellipseIn: CGRect(x: centre.x - r, y: centre.y - r, width: r * 2, height: r * 2))
        context.stroke(circle, with: .color(.white.opacity(0.12)), lineWidth: 1)

        var axes = Path()
        axes.move(to: CGPoint(x: centre.x - radius, y: centre.y))
        axes.addLine(to: CGPoint(x: centre.x + radius, y: centre.y))
        axes.move(to: CGPoint(x: centre.x, y: centre.y - radius))
        axes.addLine(to: CGPoint(x: centre.x, y: centre.y + radius))
        context.stroke(axes, with: .color(.white.opacity(0.09)), lineWidth: 1)
    }

    /// One sample is enough: skin of every shade points the same way out of the
    /// middle, and it is how far out and how bright that varies.
    private func skin(_ context: inout GraphicsContext, centre: CGPoint, radius: CGFloat) {
        let at = ScopeGraticule.position(ScopeGraticule.skin)
        let length = max((at.x * at.x + at.y * at.y).squareRoot(), 1e-4)
        let end = place(
            CGPoint(x: at.x / length * 0.95, y: at.y / length * 0.95),
            centre: centre, radius: radius)
        var path = Path()
        path.move(to: centre)
        path.addLine(to: end)
        context.stroke(
            path, with: .color(Color(red: 1, green: 0.75, blue: 0.6).opacity(0.35)), lineWidth: 1)
    }

    private func boxes(_ context: inout GraphicsContext, centre: CGPoint, radius: CGFloat) {
        for target in ScopeGraticule.targets {
            let at = place(ScopeGraticule.position(target), centre: centre, radius: radius)
            let box = CGRect(x: at.x - 4.5, y: at.y - 4.5, width: 9, height: 9)
            context.stroke(Path(box), with: .color(.white.opacity(0.35)), lineWidth: 1)
            context.draw(
                Text(target.name).font(.system(size: 9)).foregroundStyle(Color.white.opacity(0.45)),
                at: CGPoint(x: at.x + 6, y: at.y - 6),
                anchor: .bottomLeading
            )
        }
    }
}

/// Four channels, additive.
///
/// The fills add rather than blend, so where the channels agree they build into
/// a pale grey and a coloured edge showing out of the mass is a channel that
/// has drifted from the others. Painted in order they would instead leave
/// whichever is drawn last on top, and a neutral picture would read as blue.
public struct HistogramView: View {
    let levels: Scopes.Levels

    public init(levels: Scopes.Levels) {
        self.levels = levels
    }

    /// One channel smoothed and compressed into 0…1 heights.
    ///
    /// The smoothing itself is `CurveBackdrop.trace`, which is where it lives
    /// because the curve editor draws the same trace behind a curve and
    /// because that copy is checked against the engine's bin for bin. A scope
    /// counts in whole pixels, so it has a `UInt32` peak to offer and this is
    /// the one line that says so.
    public static func trace(_ bins: [UInt32], peak: UInt32) -> [Double] {
        CurveBackdrop.trace(bins, peak: Double(peak))
    }

    public var body: some View {
        Canvas { context, size in
            context.blendMode = .plusLighter
            for channel in [Scopes.Channel.red, .green, .blue] {
                let heights = Self.trace(levels.plane(channel), peak: levels.fullScale)
                let tint = ScopeImage.tint(channel)
                let colour = Color(red: tint.red, green: tint.green, blue: tint.blue)
                context.fill(area(heights, in: size), with: .color(colour.opacity(0.22)))
            }
            let luma = Self.trace(levels.plane(.luma), peak: levels.fullScale)
            context.stroke(
                top(luma, in: size), with: .color(.white.opacity(0.45)), lineWidth: 1)
        }
        .allowsHitTesting(false)
    }

    /// The filled body of one channel, from the baseline up.
    private func area(_ heights: [Double], in size: CGSize) -> Path {
        var path = top(heights, in: size)
        guard heights.count > 1 else { return path }
        path.addLine(to: CGPoint(x: size.width, y: size.height))
        path.addLine(to: CGPoint(x: 0, y: size.height))
        path.closeSubpath()
        return path
    }

    /// Its outline. Levels run left to right and counts run up.
    private func top(_ heights: [Double], in size: CGSize) -> Path {
        Path { p in
            guard heights.count > 1 else { return }
            let span = CGFloat(heights.count - 1)
            for (i, h) in heights.enumerated() {
                let point = CGPoint(
                    x: size.width * CGFloat(i) / span,
                    y: size.height * (1 - CGFloat(h) * 0.96)
                )
                if i == 0 { p.move(to: point) } else { p.addLine(to: point) }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// The panel
// -----------------------------------------------------------------------------

/// Which scopes are on screen. Several at once, like Resolve — the whole point
/// is reading one against another.
public struct ScopeSelection: Equatable, Sendable {
    public var waveform: Bool
    /// Whether the waveform overlays the three channels rather than showing
    /// luma alone. Both readings are worth having and neither replaces the
    /// other.
    public var waveformRGB: Bool
    public var parade: Bool
    public var vectorscope: Bool
    public var histogram: Bool

    /// A waveform and a vectorscope to begin with: the waveform is the only
    /// scope that says *where* in the frame something is happening, and the
    /// vectorscope is the only one that says which *direction* a cast is in.
    /// Two is also the smallest number that makes the point of the panel —
    /// one scope read against another.
    public init(
        waveform: Bool = true,
        waveformRGB: Bool = false,
        parade: Bool = false,
        vectorscope: Bool = true,
        histogram: Bool = false
    ) {
        self.waveform = waveform
        self.waveformRGB = waveformRGB
        self.parade = parade
        self.vectorscope = vectorscope
        self.histogram = histogram
    }

    public var any: Bool { waveform || parade || vectorscope || histogram }

    public var channels: [Scopes.Channel] {
        waveformRGB ? [.red, .green, .blue] : [.luma]
    }
}

/// The scopes panel.
///
/// It also tells the store it is on screen, which is what stops the engine
/// paying for a measurement nobody is looking at: a full extra render plus a
/// 1.2 MB readback, behind a closed panel, is the kind of cost nobody
/// attributes correctly later.
public struct ScopesPanel: View {
    private let store: SessionStore

    @State private var shown = ScopeSelection()

    public init(store: SessionStore) {
        self.store = store
    }

    /// What to measure at, from how wide the panel is.
    ///
    /// A waveform has one column per pixel of the measured frame, so the
    /// panel's own width is the natural number to ask for. Quantised, because
    /// a window being dragged would otherwise ask for a fresh measurement at
    /// every intermediate width — and each one is a render and a readback.
    /// The aspect is the photograph's, so what is measured is the picture
    /// resampled rather than the picture squashed.
    public static func measurement(panelWidth: CGFloat, aspect: Double) -> ScopeSize {
        let quantised = (Int(max(panelWidth, 0)) / 64) * 64
        let width = min(max(quantised, 128), 640)
        let tall = (Double(width) * (aspect > 0 ? aspect : 0.75)).rounded()
        let height = min(max(Int(tall), 96), 640)
        return ScopeSize(width: UInt32(width), height: UInt32(height))
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            picker
            content(of: store.scopes)
        }
        .padding(8)
        .background(watcher)
        .onDisappear { store.requestScopes(nil) }
    }

    // ---- the parts -------------------------------------------------------

    @ViewBuilder
    private func content(of scopes: Scopes?) -> some View {
        if let scopes {
            if shown.any {
                row(scopes)
            } else {
                note("nothing selected")
            }
        } else {
            note("no measurement yet")
        }
    }

    @ViewBuilder
    private func row(_ scopes: Scopes) -> some View {
        HStack(alignment: .top, spacing: 8) {
            if shown.waveform {
                well(shown.waveformRGB ? "Waveform · RGB" : "Waveform · Luma") {
                    WaveformView(scopes: scopes, channels: shown.channels)
                }
            }
            if shown.parade {
                well("Parade") { ParadeView(scopes: scopes) }
            }
            if shown.vectorscope {
                well("Vectorscope") { VectorscopeView(scopes: scopes) }
            }
            if shown.histogram {
                well("Histogram") { HistogramView(levels: scopes.histogram) }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// A titled, sunken box for one scope. The same shape as the curve
    /// editor's well, which is what makes the panel read as part of the same
    /// application.
    @ViewBuilder
    private func well<Content: View>(
        _ title: String, @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title)
                .font(.system(size: 10))
                .foregroundStyle(Palette.dim.color)
            content()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                // `WELL`, which is what the inside of anything read as a graph
                // is: the curve editor, the warper plots and these. It was a
                // black wash at a third opacity, which came out a different
                // grey from the curve editor's black wash at a different
                // opacity, over a different panel.
                .background(Palette.well.color)
                .clipShape(RoundedRectangle(cornerRadius: 3))
                .overlay(
                    RoundedRectangle(cornerRadius: 3)
                        .strokeBorder(Palette.rule.color, lineWidth: 1)
                )
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ViewBuilder
    private var picker: some View {
        HStack(spacing: 4) {
            Toggle("Waveform", isOn: $shown.waveform)
            Toggle("RGB", isOn: $shown.waveformRGB)
                .disabled(!shown.waveform)
            Toggle("Parade", isOn: $shown.parade)
            // "Vectorscope", not "Vector": `main.rs` calls it that, and so
            // does the well this toggle opens — a control and the panel it
            // shows should not be two different words for one thing.
            Toggle("Vectorscope", isOn: $shown.vectorscope)
            Toggle("Histogram", isOn: $shown.histogram)
            Spacer(minLength: 8)
            // What the numbers are *of*, which is not what is on screen.
            // Somebody zoomed into a corner would otherwise read the scopes as
            // describing the corner. `main.rs` says the same, in the same
            // place, for the same reason.
            //
            // Whole or absent, the same as the GPU name in the toolbar.
            // `layoutPriority(-1)` was not enough on its own: `fixedSize`
            // pins the text to its full width whatever its priority, so the
            // *toggles* gave way instead and read "W… R… P… V… Hi…".
            ViewThatFits(in: .horizontal) {
                Text("measured on the whole photograph, not the visible part")
                    .font(.system(size: 10))
                    .foregroundStyle(Palette.dim.color)
                    .lineLimit(1)
                    .fixedSize()
                Color.clear.frame(width: 0, height: 0)
            }
        }
        // `SELECT`, not `ACCENT`. Which scopes are on screen is a *choice*,
        // and `.toggleStyle(.button)` painted it in the system accent — the
        // colour Resolve reserves for the one effect that is open. Two
        // different facts, and the scheme keeps them apart.
        .toggleStyle(KromaToggleButtonStyle())
    }

    @ViewBuilder
    private func note(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 11))
            .foregroundStyle(Palette.dim.color)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// The panel's own width, without disturbing the layout — which is what a
    /// `GeometryReader` in the background buys over one wrapped round the
    /// whole thing.
    private var watcher: some View {
        GeometryReader { geo in
            Color.clear
                .onAppear { ask(width: geo.size.width) }
                .onChange(of: geo.size.width) { _, width in ask(width: width) }
                .onChange(of: aspect) { _, _ in ask(width: geo.size.width) }
        }
    }

    private var aspect: Double {
        let width = Double(store.snapshot.width)
        let height = Double(store.snapshot.height)
        return width > 0 ? height / width : 0.75
    }

    private func ask(width: CGFloat) {
        store.requestScopes(Self.measurement(panelWidth: width, aspect: aspect))
    }
}
