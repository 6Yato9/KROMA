import CoreGraphics

/// Where a value sits on a track, and where the track's fill starts and ends.
///
/// Separated from the view because this is the part with arithmetic in it, and
/// arithmetic inside a `GeometryReader` is arithmetic nobody can test. Every
/// division here is guarded: a view is laid out at zero width for at least one
/// pass, and a NaN that reaches a shape is a control that never draws again.
public struct SliderGeometry {
    public let bounds: Bounds
    public let width: CGFloat

    public init(bounds: Bounds, width: CGFloat) {
        self.bounds = bounds
        self.width = max(0, width)
    }

    private var span: Float {
        let s = bounds.max - bounds.min
        return s == 0 ? 1 : s
    }

    /// Where on the track a value sits, in points from the left.
    public func position(of value: Float) -> CGFloat {
        let t = (clamp(value) - bounds.min) / span
        return CGFloat(t) * width
    }

    /// What value a point on the track means.
    public func value(at position: CGFloat) -> Float {
        guard width > 0 else { return bounds.min }
        let t = Float(min(max(position, 0), width) / width)
        return clamp(bounds.min + t * span)
    }

    /// The filled part of the track: from the neutral point to the value.
    ///
    /// Anchored at neutral rather than at the left end, because the fill is
    /// meant to read as "how far from doing nothing", and a scale whose
    /// nothing sits in the middle would otherwise show a negative exposure as
    /// a large positive amount of something.
    public func fill(for value: Float) -> (origin: CGFloat, width: CGFloat) {
        let anchor = position(of: bounds.neutral)
        let here = position(of: value)
        return (origin: min(anchor, here), width: abs(here - anchor))
    }

    public func clamp(_ value: Float) -> Float {
        min(max(value, bounds.min), bounds.max)
    }
}
