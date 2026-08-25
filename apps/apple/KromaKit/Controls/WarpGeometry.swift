import CoreGraphics
import Foundation

/// Which two axes a lattice is being read against.
public enum WarpAxes: Sendable {
    /// Hue around, saturation out from the middle.
    case hueSat
    /// Chroma across, luma up.
    case chromaLuma

    /// Whether the first axis is a circle. Hue is: the vertex at the far right
    /// of the grid is the same vertex as the one at the far left. Chroma is
    /// not — its two ends are grey and full colour, which are as far apart as
    /// two colours get.
    public var wraps: Bool { self == .hueSat }
}

/// Where a lattice's vertices are, on screen and in axis units.
///
/// A second implementation of `Warp::home` plus the shell's own screen
/// mapping, for the same reason `CurveGeometry` duplicates the curve
/// evaluator: a drag would otherwise cost a C call per frame to answer four
/// lines of trigonometry. `home` is checked against the engine's own output by
/// `WarpGeometryTests.testEveryVertexSitsWhereTheEnginePutsIt`.
public struct WarpGeometry {
    public let warp: WarpValue
    public let axes: WarpAxes
    public let rect: CGRect

    /// How close the pointer has to be to grab a vertex, in points. Resolve's
    /// own feel, and the Windows shell's `GRAB`.
    public static let grab: CGFloat = 11

    /// How much of the plot's half-width full saturation reaches. Short of the
    /// edge so an outer vertex dragged further still has somewhere to go.
    ///
    /// Public because the haze drawn under the lattice has to land on the same
    /// disc the lattice sits on: a second copy of this number is a second copy
    /// that can drift, and a cloud on a disc of its own size is the exact
    /// failure the backdrops plan exists to prevent.
    public static let radiusFraction: CGFloat = 0.45

    public init(warp: WarpValue, axes: WarpAxes, rect: CGRect) {
        self.warp = warp
        self.axes = axes
        self.rect = rect
    }

    // ---- axis units ------------------------------------------------------

    /// Where a vertex sits before anything has been dragged.
    ///
    /// A wrapping axis has `cols` distinct positions around the ring, so the
    /// step is `1 / cols` and it never reaches 1.0 — because 1.0 is 0.0. An
    /// axis with ends has to reach both, so the step is `1 / (cols - 1)`.
    /// Using one rule for both leaves either a kink at red or a lattice that
    /// stops short of full chroma.
    public func home(col: Int, row: Int) -> CGPoint {
        let u: CGFloat = axes.wraps
            ? CGFloat(col) / CGFloat(max(warp.cols, 1))
            : CGFloat(col) / CGFloat(max(warp.cols - 1, 1))
        let v = CGFloat(row) / CGFloat(max(warp.rows - 1, 1))
        return CGPoint(x: u, y: v)
    }

    /// Where a vertex actually is — its home, displaced by its own offset.
    public func displaced(col: Int, row: Int) -> CGPoint {
        let h = home(col: col, row: row)
        let o = warp.at(col: col, row: row)
        return CGPoint(x: h.x + o.x, y: h.y + o.y)
    }

    // ---- screen ----------------------------------------------------------

    public func toScreen(_ at: CGPoint) -> CGPoint {
        switch axes {
        case .hueSat:
            let a = at.x * 2 * .pi
            let r = min(max(at.y, 0), 1) * rect.width * Self.radiusFraction
            // Minus on y because hue runs anticlockwise and view y grows down.
            return CGPoint(
                x: rect.midX + r * cos(a),
                y: rect.midY - r * sin(a)
            )
        case .chromaLuma:
            return CGPoint(
                x: rect.minX + min(max(at.x, 0), 1) * rect.width,
                // Luma up, which is the way every other plot here draws it.
                y: rect.maxY - min(max(at.y, 0), 1) * rect.height
            )
        }
    }

    public func fromScreen(_ p: CGPoint) -> CGPoint {
        switch axes {
        case .hueSat:
            let dx = p.x - rect.midX
            let dy = p.y - rect.midY
            // atan2 gives (-pi, pi]; a full turn brings the negative half round.
            let a = atan2(-dy, dx)
            let turns = (a < 0 ? a + 2 * .pi : a) / (2 * .pi)
            let r = (dx * dx + dy * dy).squareRoot()
                / max(rect.width * Self.radiusFraction, 1e-4)
            return CGPoint(x: turns, y: min(max(r, 0), 1))
        case .chromaLuma:
            return CGPoint(
                x: min(max((p.x - rect.minX) / max(rect.width, 1e-4), 0), 1),
                y: min(max((rect.maxY - p.y) / max(rect.height, 1e-4), 0), 1)
            )
        }
    }

    // ---- interaction -----------------------------------------------------

    /// The vertex nearest a point, if one is close enough to have been aimed
    /// at. Measured from where each vertex has been dragged to, because that
    /// is where it is drawn.
    public func nearest(to p: CGPoint) -> (col: Int, row: Int)? {
        var best: (col: Int, row: Int, distance: CGFloat)?
        for row in 0..<warp.rows {
            for col in 0..<warp.cols {
                let at = toScreen(displaced(col: col, row: row))
                let dx = at.x - p.x, dy = at.y - p.y
                let d = (dx * dx + dy * dy).squareRoot()
                if best == nil || d < best!.distance {
                    best = (col, row, d)
                }
            }
        }
        guard let best, best.distance <= Self.grab else { return nil }
        return (best.col, best.row)
    }

    /// The offset to store for a vertex dragged to a point.
    ///
    /// A warp stores the *difference* from where the vertex would sit if it had
    /// never been touched, so this is the drag target minus its home.
    public func offset(draggingCol col: Int, row: Int, to p: CGPoint) -> CGPoint {
        let want = fromScreen(p)
        let h = home(col: col, row: row)
        var dx = want.x - h.x
        if axes.wraps {
            // Round the hue difference the short way. Without this, dragging a
            // red vertex a little anticlockwise records almost a full turn.
            dx -= dx.rounded()
        }
        return CGPoint(
            x: min(max(dx, -0.5), 0.5),
            y: min(max(want.y - h.y, -1), 1)
        )
    }
}
