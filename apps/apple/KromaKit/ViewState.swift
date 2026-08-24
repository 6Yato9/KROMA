import CoreGraphics

/// How much of the photograph is on screen, and which part.
///
/// Kept here rather than in the engine because it is a property of the window,
/// not of the document — two windows on one photograph would disagree about it
/// and both be right. The engine is told the answer and renders that rectangle;
/// it does not have an opinion about scroll wheels.
///
/// Everything is in frame coordinates, where the whole picture is the unit
/// square, so none of it depends on how big the window happens to be.
public struct ViewState: Equatable {
    /// One is the whole frame fitted. Thirty-two is as far in as it goes —
    /// beyond that a pixel fills a screen and there is nothing left to judge.
    public static let maxZoom: CGFloat = 32

    public private(set) var zoom: CGFloat = 1
    public private(set) var pan: CGPoint = .zero

    public init() {}

    public mutating func fit() {
        zoom = 1
        pan = .zero
    }

    /// The visible rectangle, in frame coordinates.
    public var region: CGRect {
        let size = 1 / zoom
        // Clamped so the rectangle never leaves the picture. At fit there is
        // nowhere to go and the clamp collapses to zero, which is why panning a
        // fitted view does nothing.
        let slack = max(0, 1 - size)
        let x = min(max(pan.x, 0), slack)
        let y = min(max(pan.y, 0), slack)
        return CGRect(x: x, y: y, width: size, height: size)
    }

    /// Where a point of the *view* lands in the frame. The view point is a
    /// fraction of the viewport, so (0.5, 0.5) is its middle.
    public func frameLocation(of viewPoint: CGPoint) -> CGPoint {
        let r = region
        return CGPoint(
            x: r.origin.x + viewPoint.x * r.width,
            y: r.origin.y + viewPoint.y * r.height
        )
    }

    /// Zoom about a point of the view, holding whatever is under it still.
    public mutating func zoom(by factor: CGFloat, at viewPoint: CGPoint) {
        let anchor = frameLocation(of: viewPoint)
        zoom = min(max(zoom * factor, 1), Self.maxZoom)
        // Put the anchor back under the same point of the view.
        let size = 1 / zoom
        pan = CGPoint(
            x: anchor.x - viewPoint.x * size,
            y: anchor.y - viewPoint.y * size
        )
        normalise()
    }

    /// Drag the picture. The delta is in *view* fractions, so a drag across
    /// half the window moves half of whatever is on screen, at any zoom.
    public mutating func pan(by delta: CGSize) {
        let size = 1 / zoom
        pan = CGPoint(
            x: pan.x - delta.width * size,
            y: pan.y - delta.height * size
        )
        normalise()
    }

    private mutating func normalise() {
        let r = region
        pan = r.origin
    }
}
