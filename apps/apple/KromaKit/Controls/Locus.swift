import CoreGraphics
import Foundation

/// The spectral locus, and what colour sits at a chromaticity.
///
/// A mirror of `pe_color::locus`, for the reason `CurveGeometry`,
/// `WarpGeometry` and `PinGeometry` mirror theirs: a polyline and a per-texel
/// colour are not worth a round trip through the engine, and the plot is
/// rebuilt behind a slider drag. `LocusTests` holds every number here to
/// `locus.json`, which the engine writes.
///
/// Deliberately `Float`, unlike the geometry above it, because the engine's is:
/// the arithmetic below is the same operations in the same order on the same
/// 32-bit values, so the fixture can be asserted *exactly* rather than to a
/// tolerance. In `Double` every one of these numbers would be a little
/// different from the engine's and the fixture could only say "close".
public enum Locus {

    /// The CIE 1931 2° spectral locus, in xy, at 10 nm from 380 to 700 nm.
    ///
    /// The three anchors worth checking against any table you have to hand are
    /// the ends and the top: 380 nm at (0.1741, 0.0050), 520 nm at
    /// (0.0743, 0.8338) — the greenest point there is — and 700 nm at
    /// (0.7347, 0.2653).
    ///
    /// The polygon closes from 700 nm straight back to 380 nm, which is the
    /// line of purples: colours that are real but have no wavelength.
    public static let table: [SIMD2<Float>] = [
        [0.1741, 0.0050],
        [0.1738, 0.0049],
        [0.1733, 0.0048],
        [0.1726, 0.0048],
        [0.1714, 0.0051],
        [0.1689, 0.0069],
        [0.1644, 0.0109],
        [0.1566, 0.0177],
        [0.1440, 0.0297],
        [0.1241, 0.0578],
        [0.0913, 0.1327],
        [0.0454, 0.2950],
        [0.0082, 0.5384],
        [0.0139, 0.7502],
        [0.0743, 0.8338],
        [0.1547, 0.8059],
        [0.2296, 0.7543],
        [0.3016, 0.6923],
        [0.3731, 0.6245],
        [0.4441, 0.5547],
        [0.5125, 0.4866],
        [0.5752, 0.4242],
        [0.6270, 0.3725],
        [0.6658, 0.3340],
        [0.6915, 0.3083],
        [0.7079, 0.2920],
        [0.7190, 0.2809],
        [0.7260, 0.2740],
        [0.7300, 0.2700],
        [0.7320, 0.2680],
        [0.7334, 0.2666],
        [0.7344, 0.2656],
        [0.7347, 0.2653],
    ]

    /// How many points the curve is drawn and tested against, between each
    /// tabulated pair. `pe_color::locus::SUBDIVISIONS`.
    public static let subdivisions = 16

    /// The locus as a smooth closed curve.
    ///
    /// Catmull-Rom through the tabulated points: it passes *through* every one
    /// of them, which matters when the points are measurements rather than
    /// handles. `CurveGeometry` rejected Catmull-Rom for exactly the property
    /// that is harmless here — it overshoots between control points, and a tone
    /// curve that bulges is a bright halo nobody asked for. This is a smooth
    /// closed curve that no pixel is looked up in. The choice is deliberate on
    /// both sides; neither should be changed to match the other.
    ///
    /// The line of purples is not in here. It is the chord from the last point
    /// back to the first, which is what closing the path draws — a chord and
    /// not a spectral colour, and rounding it off with the spline would claim
    /// colours that do not exist.
    ///
    /// Built once for the process, because it is the same 513 points every
    /// time: a `static let` is the whole of the memo `WarperCloudMemo` needs a
    /// key for, since this has no input to key on.
    public static let curve: [SIMD2<Float>] = {
        let n = table.count
        let at = { (i: Int) in table[Swift.min(Swift.max(i, 0), n - 1)] }
        var out: [SIMD2<Float>] = []
        out.reserveCapacity(n * subdivisions + 1)
        for i in 0..<(n - 1) {
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2))
            for step in 0..<subdivisions {
                let t = Float(step) / Float(subdivisions)
                out.append(catmullRom(p0, p1, p2, p3, t))
            }
        }
        out.append(table[n - 1])
        return out
    }()

    private static func catmullRom(
        _ p0: SIMD2<Float>, _ p1: SIMD2<Float>, _ p2: SIMD2<Float>, _ p3: SIMD2<Float>,
        _ t: Float
    ) -> SIMD2<Float> {
        let t2 = t * t
        let t3 = t * t * t
        let axis = { (a: Float, b: Float, c: Float, d: Float) -> Float in
            0.5
                * ((2.0 * b)
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (-a + 3.0 * b - 3.0 * c + d) * t3)
        }
        return SIMD2(
            axis(p0.x, p1.x, p2.x, p3.x),
            axis(p0.y, p1.y, p2.y, p3.y)
        )
    }

    // ---- is this a colour at all -----------------------------------------

    /// How finely the span table divides the y axis.
    private static let spanRows = 1024
    /// The top of the table. The locus reaches 0.8338.
    private static let spanTop: Float = 0.84

    /// For each row of y, where the curve starts and stops.
    ///
    /// The region is convex, so a horizontal line crosses its boundary exactly
    /// twice and "is this a colour" becomes a lookup and two comparisons. Built
    /// because ``inside(_:_:)`` is asked once per texel of the field — sixty-five
    /// thousand times — and walking five hundred segments each time is a
    /// visible pause on the first draw of the panel.
    private static let spans: [SIMD2<Float>] = {
        var rows = [SIMD2<Float>](repeating: [1.0, -1.0], count: spanRows)
        let points = curve
        let n = points.count
        for row in 0..<spanRows {
            let y = (Float(row) + 0.5) / Float(spanRows) * spanTop
            var lo = Float.greatestFiniteMagnitude
            var hi = -Float.greatestFiniteMagnitude
            for i in 0..<n {
                // Closed: the last point joins the first along the purple line.
                let a = points[i]
                let b = points[(i + 1) % n]
                if (a.y > y) != (b.y > y) {
                    let t = (y - a.y) / (b.y - a.y)
                    let x = a.x + t * (b.x - a.x)
                    lo = Swift.min(lo, x)
                    hi = Swift.max(hi, x)
                }
            }
            rows[row] = lo <= hi ? [lo, hi] : [1.0, -1.0]
        }
        return rows
    }()

    /// Whether a chromaticity is a colour at all.
    public static func inside(_ x: Float, _ y: Float) -> Bool {
        guard y >= 0, y < spanTop else { return false }
        let row = Int((y / spanTop) * Float(spanRows))
        let span = spans[Swift.min(row, spanRows - 1)]
        return x >= span.x && x <= span.y
    }

    // ---- and what it looks like ------------------------------------------

    /// The sRGB primaries as a matrix from XYZ.
    ///
    /// The engine derives these nine numbers from `pe_color::primaries::SRGB`
    /// rather than writing them out, because that crate holds the four
    /// chromaticities they are a consequence of. This side holds no primaries
    /// and no matrix inverse, so it carries the result — the same trade every
    /// mirror in this directory makes, and the fixture is what keeps it from
    /// drifting: these are the engine's `f32` values to the last bit, and
    /// `testEveryProbeIsTheColourTheEngineDraws` compares the colours they
    /// produce *exactly*, so a digit changed here is a test failure and not a
    /// slightly different plot.
    private static let xyzToSrgb: [SIMD3<Float>] = [
        [3.2404542, -1.53713846, -0.498531401],
        [-0.969266057, 1.87601089, 0.0415560193],
        [0.0556434318, -0.204025909, 1.05722523],
    ]

    /// The colour at a chromaticity, as near as a display can put it.
    ///
    /// Answered for the *whole plane*, not only for real colours — the caller
    /// asks ``inside(_:_:)`` separately and dims what is outside. A black
    /// surround makes the plot a shape floating in nothing, where a dimmed one
    /// makes it a bright region of a continuous field, which is what a gamut
    /// actually is.
    ///
    /// Nil only where the arithmetic has nothing to say: at y of zero there is
    /// no colour to normalise, however the plot would like to draw it.
    ///
    /// Clipped towards white rather than per channel, and normalised to full
    /// brightness rather than scaled by luminance. Both are the engine's, and
    /// both are the difference between a map of chromaticity and a picture of
    /// one display's gamut with the edges gone wrong.
    public static func colour(at p: SIMD2<Float>) -> SIMD3<Float>? {
        let (x, y) = (p.x, p.y)
        guard y > 1e-4 else { return nil }
        let xyz = SIMD3<Float>(x / y, 1.0, (1.0 - x - y) / y)
        var rgb = SIMD3<Float>()
        for i in 0..<3 {
            let row = xyzToSrgb[i]
            rgb[i] = row[0] * xyz[0] + row[1] * xyz[1] + row[2] * xyz[2]
        }
        // Clipped towards white rather than per channel: taking a negative to
        // zero on its own shifts the hue, and a plot whose greens turn cyan at
        // the edge is worse than one whose greens go pale.
        var low: Float = 0
        for c in 0..<3 { low = Swift.min(low, rgb[c]) }
        if low < 0 {
            for c in 0..<3 { rgb[c] -= low }
        }
        var peak: Float = 1e-4
        for c in 0..<3 { peak = Swift.max(peak, rgb[c]) }
        for c in 0..<3 { rgb[c] /= peak }
        return rgb
    }
}

// -----------------------------------------------------------------------------
// The field of colour the plot is a map of
// -----------------------------------------------------------------------------

extension Locus {

    /// How many texels across the field is built.
    ///
    /// The same 256 `WarperCloud` uses, because the two are drawn over the same
    /// square at the same size — two images built at different resolutions over
    /// one plot beat against each other where their texel edges disagree.
    public static let texels = 256

    /// How bright a real colour is drawn, and how bright the rest is.
    ///
    /// The Windows shell's `plot_image`. The bright half is still well under
    /// full: the plot is a map and the photograph's own colours are what you
    /// came to look at, and at full brightness the map wins.
    public static let insideLevel: Float = 0.62
    /// What is outside the horseshoe is *dimmed*, not blackened. That is the
    /// whole argument: a black surround makes the plot a shape floating in
    /// nothing; a dimmed one makes it a bright region of a continuous field,
    /// which is what a gamut actually is.
    public static let outsideLevel: Float = 0.16

    /// What is drawn where there is no colour to normalise — the strip below
    /// y = 0, which the plot reaches because `PinGeometry.plotMin` is a hair
    /// under zero. The Windows shell's near-black, and near-black rather than
    /// black so the strip still reads as part of the field.
    static let nothingAtAll = SIMD3<Float>(0.03, 0.03, 0.035)

    /// The whole plot as one image: every chromaticity it covers, dimmed
    /// outside the horseshoe.
    ///
    /// One image rather than sixty-five thousand rectangles, for the reason
    /// `WarperCloud.image` and the scopes give — and one image rather than a
    /// fill plus a mask, because the dimming is per texel and a shape has no
    /// way to say "this colour, at 0.16".
    ///
    /// A `static let`: the field depends on nothing but the locus and
    /// `PinGeometry`'s range, so it is built once for the process rather than
    /// keyed on a generation the way a cloud is.
    ///
    /// Mapped through `PinGeometry` rather than through a second copy of the
    /// plot's range. A field built over a range of its own would be a picture
    /// of the right colours in the wrong places, with the pins landing
    /// somewhere else again.
    public static let field: CGImage? = buildField()

    private static func buildField() -> CGImage? {
        let n = texels
        var bytes = [UInt8](repeating: 0, count: n * n * 4)
        for row in 0..<n {
            // Texel centres, and v measured upwards to match every plot here.
            let y = Float(PinGeometry.value(at: 1 - (Double(row) + 0.5) / Double(n)))
            for col in 0..<n {
                let x = Float(PinGeometry.value(at: (Double(col) + 0.5) / Double(n)))
                var rgb = nothingAtAll
                if let c = colour(at: SIMD2(x, y)) {
                    rgb = c * (inside(x, y) ? insideLevel : outsideLevel)
                }
                let i = (row * n + col) * 4
                for k in 0..<3 {
                    bytes[i + k] = UInt8(Swift.min(Swift.max(rgb[k], 0), 1) * 255)
                }
                bytes[i + 3] = 255
            }
        }
        guard let provider = CGDataProvider(data: Data(bytes) as CFData) else { return nil }
        return CGImage(
            width: n, height: n,
            bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: n * 4,
            // sRGB by name rather than the device's: these bytes came out of
            // the sRGB matrix above, so that is the space they mean.
            space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent
        )
    }
}
