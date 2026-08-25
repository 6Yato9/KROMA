import SwiftUI

/// Resolve's four-way wheel: a colour disc with a handle, and a ribbed bar
/// under it for the achromatic push.
///
/// `master` on the registry entry says whether the wheel has a fourth
/// *readout*, not whether it has an achromatic control — every wheel has the
/// bar. Offset is the case that makes the distinction: Resolve draws four bars
/// and three of Offset's boxes, and on a wheel with no master the bar moves the
/// three channels together.
public struct WheelView: View {
    let param: Param
    let bounds: Bounds
    let hasMaster: Bool
    let row: UInt64
    let value: WheelValue
    let isActive: Bool
    let store: SessionStore

    @State private var dragging: WheelValue?

    private var shown: WheelValue { dragging ?? value }

    public init(
        param: Param, bounds: Bounds, hasMaster: Bool, row: UInt64,
        value: WheelValue, isActive: Bool, store: SessionStore
    ) {
        self.param = param
        self.bounds = bounds
        self.hasMaster = hasMaster
        self.row = row
        self.value = value
        self.isActive = isActive
        self.store = store
    }

    private static let size: CGFloat = 84

    public var body: some View {
        VStack(spacing: 4) {
            Text(param.name)
                .font(.system(size: 11))
                .foregroundStyle(Palette.label.color)

            disc

            bar

            Text(readout)
                .font(.system(size: 10))
                .monospacedDigit()
                .foregroundStyle(Palette.dim.color)
        }
        .frame(width: Self.size + 16)
        .opacity(isActive ? 1 : ScalarRow.dimmed)
        .disabled(!isActive)
    }

    // ---- the disc's colours ----------------------------------------------

    /// Where a wheel angle falls on the disc, as a fraction clockwise from
    /// three o'clock — which is where an `AngularGradient` starts and the
    /// direction it sweeps, because view y grows downwards and the wheel's
    /// does not.
    static func sweep(forWheelAngle degrees: Double) -> Double {
        var a = degrees.truncatingRemainder(dividingBy: 360)
        if a < 0 { a += 360 }
        return (360 - a).truncatingRemainder(dividingBy: 360) / 360
    }

    /// The hue painted at a fraction round the disc.
    ///
    /// Taken from ``Ramp/hue`` rather than from SwiftUI's saturated primaries,
    /// so the disc is the same hue circle every Hue track in the application
    /// draws and the same one the engine's fixture checks.
    ///
    /// And laid on so that each channel's colour sits at that channel's own
    /// angle: `WheelGeometry` puts red up, green at two hundred and ten
    /// degrees and blue at three hundred and thirty. Painted in gradient order
    /// from three o'clock the disc had red at the right — so a handle dragged
    /// into what looked like the reds raised something else, and the picture
    /// under the pointer disagreed with the numbers beside it.
    static func discHue(atSweep s: Double) -> Rgb8 {
        // Hue h sits at wheel angle `redAngle + h·360`; the sweep fraction s
        // is at wheel angle `-s·360`. Solving the two gives h = -s - ¼.
        var h = (-s - WheelGeometry.redAngle / 360).truncatingRemainder(dividingBy: 1)
        if h < 0 { h += 1 }
        return Ramp.hue.at(h)
    }

    private static let discStops: [Gradient.Stop] = (0...48).map { i in
        let t = Double(i) / 48
        return Gradient.Stop(color: discHue(atSweep: t).color, location: t)
    }

    private static var discGradient: AngularGradient {
        AngularGradient(gradient: Gradient(stops: discStops), center: .center)
    }

    private var readout: String {
        // The master's box, where there is one, and otherwise the three
        // channels — which is what Resolve shows and for the same reason: the
        // bar is a nudge you make without looking, the box is a value you read.
        if hasMaster {
            return String(format: "%.3f", shown.master)
        }
        return shown.rgb.map { String(format: "%.2f", $0) }.joined(separator: " ")
    }

    private var disc: some View {
        GeometryReader { geo in
            let radius = min(geo.size.width, geo.size.height) / 2
            let g = WheelGeometry(bounds: bounds, radius: radius)
            let centre = CGPoint(x: geo.size.width / 2, y: geo.size.height / 2)
            let handle = g.point(for: shown.rgb)

            ZStack {
                Circle()
                    .fill(Self.discGradient)
                    // Held back from full strength: the disc is a backdrop a
                    // handle is read against, and Resolve's are muted for the
                    // same reason its tracks are.
                    .opacity(0.55)
                Circle().strokeBorder(Palette.rule.color, lineWidth: 1)
                Circle()
                    .fill(dragging == nil ? Palette.handle.color : Palette.handleHot.color)
                    .frame(width: 7, height: 7)
                    .overlay(Circle().strokeBorder(Palette.handleEdge.color, lineWidth: 1))
                    .position(x: centre.x + handle.x, y: centre.y - handle.y)
            }
            .contentShape(Circle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        if dragging == nil { store.beginInteraction(param.name) }
                        // Y is flipped: the view grows downward, the wheel does
                        // not.
                        let p = CGPoint(
                            x: drag.location.x - centre.x,
                            y: centre.y - drag.location.y
                        )
                        let rgb = g.rgb(at: p)
                        let next = WheelValue(rgb: rgb, master: shown.master)
                        dragging = next
                        store.setWheel(
                            row: row, key: param.key, master: next.master,
                            rgb[0], rgb[1], rgb[2]
                        )
                    }
                    .onEnded { _ in
                        store.endInteraction()
                        dragging = nil
                    }
            )
        }
        .frame(width: Self.size, height: Self.size)
    }

    /// The achromatic bar. On a wheel with no master it moves the three
    /// channels together, which is what Resolve does with Offset.
    ///
    /// Drawn with `ScalarRow`'s own arithmetic and its own colours: the same
    /// track, the same fill out of neutral, the same pointer. A wheel's bar
    /// that is a capsule with a disc on it, sitting above thirty rows that are
    /// not, reads as a control from a different application.
    private var bar: some View {
        GeometryReader { geo in
            let width = geo.size.width
            let value = hasMaster ? shown.master : (shown.rgb.first ?? bounds.neutral)
            let filled = ScalarRow.trackGeometry(bounds: bounds, over: width).fill(for: value)
            let x = ScalarRow.trackPosition(of: value, bounds: bounds, over: width)
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 2)
                    .fill(Palette.track.color)
                    .frame(width: width, height: ScalarRow.barHeight)
                if filled.width > 0.5 {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(Palette.trackFill.color)
                        .frame(width: filled.width, height: ScalarRow.barHeight)
                        .offset(x: ScalarRow.handleHalfWidth + filled.origin)
                }
                Pointer()
                    .fill(dragging == nil ? Palette.handle.color : Palette.handleHot.color)
                    .overlay(Pointer().stroke(Palette.handleEdge.color, lineWidth: 1))
                    .frame(width: ScalarRow.handleWidth, height: ScalarRow.handleHeight)
                    .offset(x: x - ScalarRow.handleHalfWidth, y: ScalarRow.handleRise)
            }
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        if dragging == nil { store.beginInteraction(param.name) }
                        let v = ScalarRow.valueOnTrack(
                            bounds: bounds, at: drag.location.x, over: width)
                        let next = hasMaster
                            ? WheelValue(rgb: shown.rgb, master: v)
                            : WheelValue(rgb: [v, v, v], master: v)
                        dragging = next
                        store.setWheel(
                            row: row, key: param.key, master: next.master,
                            next.rgb[0], next.rgb[1], next.rgb[2]
                        )
                    }
                    .onEnded { _ in
                        store.endInteraction()
                        dragging = nil
                    }
            )
        }
        .frame(width: Self.size, height: 14)
    }
}
