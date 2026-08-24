import CoreGraphics
import Foundation

/// Where a point in a colour wheel sits, as three channel values.
///
/// Separated from the view for the same reason `SliderGeometry` is: this is the
/// part with arithmetic in it, and arithmetic inside a `GeometryReader` cannot
/// be tested. Every division is guarded — a view is laid out at zero size for
/// at least one pass, and a NaN that reaches a `Shape` is a control that never
/// draws again.
///
/// The channels sit where Resolve puts them: red up, green at two hundred and
/// ten degrees, blue at three hundred and thirty. Pulling towards one raises it
/// and lowers the other two by half as much each, so the three always sum to
/// nothing — a colour wheel moves hue and leaves level alone, and the ribbed
/// bar beside it is what moves level.
public struct WheelGeometry {
    public let bounds: Bounds
    public let radius: CGFloat

    public static let redAngle: Double = 90
    public static let greenAngle: Double = 210
    public static let blueAngle: Double = 330

    public init(bounds: Bounds, radius: CGFloat) {
        self.bounds = bounds
        self.radius = max(0, radius)
    }

    /// How far a push at the rim moves a channel.
    ///
    /// A quarter of the range, not the whole of it: a wheel is for the small
    /// adjustments that make a grade, and one that swung a channel from end to
    /// end across fifty points of travel would be unusable for them. Gain's
    /// range runs to sixteen, and nobody nudges a wheel expecting four.
    private var reach: Float {
        (bounds.max - bounds.min) / 4
    }

    private func offset(at point: CGPoint) -> (angle: Double, amount: Float) {
        guard radius > 0 else { return (0, 0) }
        let dx = Double(point.x)
        let dy = Double(point.y)
        let distance = min((dx * dx + dy * dy).squareRoot(), Double(radius))
        guard distance > 0 else { return (0, 0) }
        var angle = atan2(dy, dx) * 180 / .pi
        if angle < 0 { angle += 360 }
        return (angle, Float(distance / Double(radius)) * reach)
    }

    /// The three channels for a point, measured from the centre.
    public func rgb(at point: CGPoint) -> [Float] {
        let (angle, amount) = offset(at: point)
        guard amount != 0 else {
            return [bounds.neutral, bounds.neutral, bounds.neutral]
        }
        // Each channel gets the cosine of its own angle away from the push,
        // which peaks at the channel being pulled towards and is negative on
        // the far side. Cosines a hundred and twenty degrees apart sum to zero,
        // which is what keeps the level still.
        return [Self.redAngle, Self.greenAngle, Self.blueAngle].map { channel in
            let d = (angle - channel) * .pi / 180
            return bounds.neutral + amount * Float(cos(d))
        }
    }

    /// The inverse: where the handle sits for a set of channel values.
    public func point(for rgb: [Float]) -> CGPoint {
        guard rgb.count == 3, radius > 0 else { return .zero }
        // Sum the three channel vectors. Two thirds because each channel's
        // contribution was a cosine, and three cosines average to two thirds of
        // the amplitude.
        var x = 0.0
        var y = 0.0
        for (value, channel) in zip(rgb, [Self.redAngle, Self.greenAngle, Self.blueAngle]) {
            let magnitude = Double(value - bounds.neutral)
            x += magnitude * cos(channel * .pi / 180)
            y += magnitude * sin(channel * .pi / 180)
        }
        let scale = Double(radius) / Double(max(reach, 1e-6)) * (2.0 / 3.0)
        return CGPoint(x: x * scale, y: y * scale)
    }

    /// A point at an angle and a distance from the centre, for the tests and
    /// for drawing the handle.
    public func point(forAngle degrees: Double, distance: CGFloat) -> CGPoint {
        let d = min(distance, radius)
        let r = degrees * .pi / 180
        return CGPoint(x: CGFloat(cos(r)) * d, y: CGFloat(sin(r)) * d)
    }
}
