import CoreGraphics
import Foundation

/// A curve, and the arithmetic for drawing and editing one.
///
/// This is a second implementation of `crates/pe-core/src/curve.rs`, which is
/// a thing worth being uncomfortable about — so it is checked against the first
/// at every one of the LUT's 256 positions, from a fixture that side generates.
/// See `CurveGeometryTests.testEveryCurveMatchesTheEngineAtEveryLutPosition`.
///
/// The alternative was asking the engine to bake on every frame of a drag,
/// which puts a C call and a 256-float copy inside a gesture to avoid forty
/// lines of arithmetic a fixture can prove correct.
///
/// **Monotone cubic Hermite, Fritsch–Carlson.** Not Catmull-Rom: that
/// overshoots between control points, so dragging a highlight down can make the
/// curve bulge above where it started somewhere in the middle. On a tone curve
/// that is a bright halo in a band the user never touched.
public struct CurveGeometry: Equatable {
    /// In x order, always. Everything that evaluates a curve needs them sorted,
    /// and sorting late is how a point dragged past its neighbour swaps places
    /// with it mid-gesture.
    public let points: [CGPoint]

    public init(points: [CGPoint]) {
        self.points = points.sorted { $0.x < $1.x }
    }

    // ---- evaluation ------------------------------------------------------

    /// Secant slopes, limited so no segment can overshoot.
    private var tangents: [Double] {
        let n = points.count
        guard n >= 2 else { return [] }

        var secants = [Double](repeating: 0, count: n - 1)
        for i in 0..<(n - 1) {
            let dx = Double(points[i + 1].x - points[i].x)
            secants[i] = abs(dx) < .ulpOfOne
                ? 0
                : Double(points[i + 1].y - points[i].y) / dx
        }

        var m = [Double](repeating: 0, count: n)
        m[0] = secants[0]
        m[n - 1] = secants[n - 2]
        for i in 1..<(n - 1) {
            // A local extremum gets a flat tangent, which is what stops the
            // curve wandering past the point the user placed.
            m[i] = secants[i - 1] * secants[i] <= 0
                ? 0
                : (secants[i - 1] + secants[i]) * 0.5
        }

        // The Fritsch–Carlson condition. Without it the cubic can overshoot
        // even with correctly signed tangents.
        for i in 0..<(n - 1) {
            if abs(secants[i]) < .ulpOfOne {
                m[i] = 0
                m[i + 1] = 0
                continue
            }
            let a = m[i] / secants[i]
            let b = m[i + 1] / secants[i]
            let s = a * a + b * b
            if s > 9 {
                let t = 3 / s.squareRoot()
                m[i] = t * a * secants[i]
                m[i + 1] = t * b * secants[i]
            }
        }
        return m
    }

    /// Evaluate at `x`, clamped to 0...1.
    public func sample(at x: Double) -> Double {
        guard points.count >= 2 else { return min(max(x, 0), 1) }
        let x = min(max(x, 0), 1)

        // Outside the control range the curve holds its endpoint, matching what
        // is drawn.
        if x <= Double(points[0].x) { return min(max(Double(points[0].y), 0), 1) }
        if x >= Double(points[points.count - 1].x) {
            return min(max(Double(points[points.count - 1].y), 0), 1)
        }

        let m = tangents
        var i = 0
        for j in 0..<(points.count - 1)
        where x >= Double(points[j].x) && x <= Double(points[j + 1].x) {
            i = j
            break
        }

        let x0 = Double(points[i].x)
        let y0 = Double(points[i].y)
        let x1 = Double(points[i + 1].x)
        let y1 = Double(points[i + 1].y)
        let h = x1 - x0
        if abs(h) < .ulpOfOne { return min(max(y1, 0), 1) }

        let t = (x - x0) / h
        let t2 = t * t
        let t3 = t2 * t
        let h00 = 2 * t3 - 3 * t2 + 1
        let h10 = t3 - 2 * t2 + t
        let h01 = -2 * t3 + 3 * t2
        let h11 = t3 - t2

        let y = h00 * y0 + h10 * h * m[i] + h01 * y1 + h11 * h * m[i + 1]
        return min(max(y, 0), 1)
    }

    // ---- editing ---------------------------------------------------------

    public func adding(_ point: CGPoint) -> CurveGeometry {
        CurveGeometry(points: points + [clampToUnit(point)])
    }

    /// Whether a point may be taken out. The ends may not: they anchor the
    /// range, and the evaluator holds an endpoint's value across the gap beyond
    /// it, so removing one shows as a flat shelf the user did not ask for.
    public func canRemovePoint(at index: Int) -> Bool {
        index > 0 && index < points.count - 1
    }

    public func removing(at index: Int) -> CurveGeometry {
        guard canRemovePoint(at: index) else { return self }
        var p = points
        p.remove(at: index)
        return CurveGeometry(points: p)
    }

    /// Move a point, keeping it inside the square and between its neighbours.
    ///
    /// An endpoint keeps its x. Letting the ends slide inward would shorten the
    /// curve and produce the same shelf as removing one.
    public func moving(at index: Int, to location: CGPoint) -> CurveGeometry {
        guard points.indices.contains(index) else { return self }
        var p = points
        let target = clampToUnit(location)

        if index == 0 || index == p.count - 1 {
            p[index] = CGPoint(x: p[index].x, y: target.y)
            return CurveGeometry(points: p)
        }

        // A hair inside the neighbours, so two points never share an x and the
        // sort cannot reorder the one being held.
        let gap: CGFloat = 0.001
        let low = p[index - 1].x + gap
        let high = p[index + 1].x - gap
        let x = low <= high ? min(max(target.x, low), high) : p[index].x
        p[index] = CGPoint(x: x, y: target.y)
        return CurveGeometry(points: p)
    }

    /// The index of the point nearest a location, if one is within `reach`.
    public func indexOfPoint(near location: CGPoint, within reach: CGFloat) -> Int? {
        var best: (index: Int, distance: CGFloat)?
        for (i, p) in points.enumerated() {
            let dx = p.x - location.x
            let dy = p.y - location.y
            let d = (dx * dx + dy * dy).squareRoot()
            if d <= reach, best == nil || d < best!.distance {
                best = (i, d)
            }
        }
        return best?.index
    }

    private func clampToUnit(_ p: CGPoint) -> CGPoint {
        CGPoint(x: min(max(p.x, 0), 1), y: min(max(p.y, 0), 1))
    }
}
