import Foundation

/// What a control wants drawn behind it, and the arithmetic for drawing it.
///
/// A mirror of `crates/pe-scopes/src/backdrop.rs`, which is a thing worth being
/// uncomfortable about — so the parts with numbers in them are checked against
/// that side at every bin, from a fixture it generates. See
/// `CurveBackdropTests.testTheTraceMatchesTheEngineAtEveryBin`.
///
/// A curve editor with nothing behind it is a diagram of a function: you can
/// see the shape you drew and not the thing you drew it for. What goes behind
/// it has to be counted in the same units its x-axis is indexed by — a tone
/// histogram behind a Hue Vs Sat curve puts every peak in the wrong place,
/// which is worse than drawing nothing, because it aims the user at colours
/// that are not there.
public enum CurveBackdrop: Equatable, Sendable {
    /// The three channel histograms, read through the SDR window.
    case tones
    /// The luma histogram alone, for a curve indexed by luminance.
    case luma
    /// Hue counts, running once round the circle from red.
    case hue
    /// Saturation counts.
    case saturation
    /// Nothing is known to belong there.
    case nothing

    /// What belongs behind the curve at `key`.
    ///
    /// Decided by what the curve's x-axis is indexed by, which is not always
    /// what its name leads with: `lum_vs_sat` reads an input *luminance* and
    /// outputs a saturation, so it takes a luma backdrop and not a saturation
    /// one. It is the case that was wrong in the other shell, and the reason
    /// this table is generated from the engine's rather than typed twice.
    public static func behind(_ key: String) -> CurveBackdrop {
        switch key {
        case "luma", "red", "green", "blue": .tones
        case "hue_vs_hue", "hue_vs_sat", "hue_vs_lum": .hue
        case "sat_vs_sat", "sat_vs_lum": .saturation
        case "lum_vs_sat": .luma
        default: .nothing
        }
    }

    // ---- the window ------------------------------------------------------

    /// Where diffuse black and diffuse white sit in the log domain the curve
    /// operates on. `pe_core::parametric::LOG_BLACK` and `LOG_WHITE`, widened.
    ///
    /// A tone plot spans these two rather than the whole of ACEScct, because
    /// the rest is headroom above diffuse white — real signal, where a
    /// recovered highlight lives, but not what the plot is drawn over.
    public static let logBlack = 0.072_905_533_015_728
    public static let logWhite = 0.554_794_490_337_371_8

    /// Bins per histogram. Matches an 8-bit display, and finer bins do not
    /// survive being drawn at panel width.
    public static let binCount = 256

    /// Which bin a fraction across a plot reads from, when the plot's axis
    /// runs edge to edge — a hue once round the circle, a saturation from
    /// nothing to full. Both fill their plot, so neither is windowed.
    public static func spreadBin(atPlotFraction fraction: Double) -> Int {
        let t = min(max(fraction, 0), 1)
        return min(Int((t * Double(binCount - 1)).rounded()), binCount - 1)
    }

    /// Which bin a fraction across a *tone* plot reads from.
    ///
    /// The plot's left edge is diffuse black and its right edge diffuse white,
    /// not bin zero and bin 255. Laid out edge to edge instead, every tone
    /// would sit about a seventh of the plot to the left of where the curve
    /// acts on it — close enough to look plausible, and wrong everywhere.
    public static func bin(atPlotFraction fraction: Double) -> Int {
        let t = min(max(fraction, 0), 1)
        return spreadBin(atPlotFraction: logBlack + t * (logWhite - logBlack))
    }

    // ---- the smoothing ---------------------------------------------------

    /// How far either side of a bin the smoothing reaches.
    ///
    /// A histogram of a photograph is spiky — real images have runs of
    /// identical values, and every one of them is a bin standing alone. Drawn
    /// raw that reads as a bar chart, which is a picture of the sampling
    /// rather than of the photograph. Three bins either side is enough to make
    /// it a curve and short enough that a genuine spike is still a spike.
    public static let smooth = 3

    /// One channel smoothed and compressed into 0…1 heights.
    ///
    /// A bin near either end has fewer neighbours, and the weight it divides
    /// by shrinks with the window rather than the window being clamped or
    /// wrapped. That matters: clamping would repeat the end bin and pull a
    /// peak outward, wrapping would fold the shadows into the highlights, and
    /// all three look identical in the middle — which is why the fixture this
    /// is checked against puts a value hard against each end.
    ///
    /// The power is the same argument as the waveform's square root: one flat
    /// area of sky can hold a fifth of the frame in a single bin, and against
    /// that everything else would be a pixel high.
    public static func trace(_ bins: [UInt32], peak: Double) -> [Double] {
        // An empty frame has no scale to draw against, and dividing by its
        // peak would make every bin NaN — which draws as a hole rather than as
        // the nothing it is. Counts are whole numbers, so a peak below one is
        // no peak.
        let full = max(peak, 1)
        let count = bins.count
        return (0..<count).map { i in
            var sum = 0.0
            var weight = 0.0
            for d in -smooth...smooth {
                let j = i + d
                guard j >= 0, j < count else { continue }
                // Triangular, which is a box filter applied twice and quite
                // smooth enough for something drawn a few hundred points wide.
                let w = 1 - Double(abs(d)) / Double(smooth + 1)
                sum += Double(bins[j]) * w
                weight += w
            }
            let v = sum / max(weight, 1e-4) / full
            return pow(min(max(v, 0), 1), 0.42)
        }
    }
}
