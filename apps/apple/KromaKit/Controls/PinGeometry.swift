import CoreGraphics
import Foundation

/// Where pins are on the chromaticity plot.
///
/// A second implementation of `pe_core::pins::plot_fraction` and its inverse,
/// for the reason `CurveGeometry` and `WarpGeometry` duplicate theirs: a drag
/// would otherwise cost a C call per frame. Checked against the engine's own
/// output by `PinGeometryTests`.
public struct PinGeometry {
    public let pins: [PinValue]
    public let rect: CGRect

    /// How far the plot reaches, in xy. The locus reaches 0.8338 in y — a span
    /// of 0.8 quietly cut the top off the curve, and the part it cut was the
    /// greenest colour there is.
    public static let plotSpan: Double = 0.88
    /// And where it starts. A hair below zero, so the locus has air around it
    /// rather than sitting hard against the frame.
    public static let plotMin: Double = -0.03

    /// How close the pointer has to be to grab a pin, in points. The Windows
    /// shell's `GRAB`, and `WarpGeometry`'s.
    public static let grab: CGFloat = 11

    public init(pins: [PinValue], rect: CGRect) {
        self.pins = pins
        self.rect = rect
    }

    /// A chromaticity as a fraction across the plot.
    public static func fraction(of v: Double) -> Double {
        min(max((v - plotMin) / (plotSpan - plotMin), 0), 1)
    }

    /// And back. Clamped, so a pin dragged past the frame stops at it rather
    /// than acquiring a chromaticity no colour has.
    public static func value(at t: Double) -> Double {
        plotMin + min(max(t, 0), 1) * (plotSpan - plotMin)
    }

    public func screen(of at: CGPoint) -> CGPoint {
        CGPoint(
            x: rect.minX + Self.fraction(of: at.x) * rect.width,
            // y up, which is how a chromaticity diagram is always drawn.
            y: rect.maxY - Self.fraction(of: at.y) * rect.height
        )
    }

    public func chromaticity(_ p: CGPoint) -> CGPoint {
        CGPoint(
            x: Self.value(at: (p.x - rect.minX) / max(rect.width, 1e-4)),
            y: Self.value(at: (rect.maxY - p.y) / max(rect.height, 1e-4))
        )
    }

    /// How far a pin reaches, in points. `chroma_range` is a distance in xy —
    /// the same units as `at` and `to` — so it is divided by the plot's own
    /// width rather than run through `fraction(of:)`.
    public func reach(chromaRange: Double) -> CGFloat {
        chromaRange / (Self.plotSpan - Self.plotMin) * rect.width
    }

    /// The pin under a point, if one is close enough to have been aimed at.
    ///
    /// Measured from the handle — where the pin has been dragged to — because
    /// that is the thing you move. The origin is a ring saying where the colour
    /// was, and grabbing it would move the wrong end.
    public func grabbed(at p: CGPoint) -> Int? {
        var best: (index: Int, distance: CGFloat)?
        for (i, pin) in pins.enumerated() {
            let s = screen(of: pin.to)
            let dx = s.x - p.x, dy = s.y - p.y
            let d = (dx * dx + dy * dy).squareRoot()
            if best == nil || d < best!.distance { best = (i, d) }
        }
        guard let best, best.distance <= Self.grab else { return nil }
        return best.index
    }
}
