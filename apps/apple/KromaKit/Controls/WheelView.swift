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
                .font(.caption)
                .foregroundStyle(isActive ? .secondary : .tertiary)

            disc

            bar

            Text(readout)
                .font(.caption2)
                .monospacedDigit()
                .foregroundStyle(isActive ? .secondary : .tertiary)
        }
        .frame(width: Self.size + 16)
        .disabled(!isActive)
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
                    .fill(
                        AngularGradient(
                            colors: [.red, .yellow, .green, .cyan, .blue, .purple, .red],
                            center: .center
                        )
                    )
                    .opacity(isActive ? 0.55 : 0.2)
                Circle().strokeBorder(.quaternary, lineWidth: 1)
                Circle()
                    .fill(.white)
                    .frame(width: 7, height: 7)
                    .overlay(Circle().strokeBorder(.black.opacity(0.6), lineWidth: 1))
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
    private var bar: some View {
        GeometryReader { geo in
            let g = SliderGeometry(bounds: bounds, width: geo.size.width)
            let position = g.position(of: hasMaster ? shown.master : shown.rgb[0])
            ZStack(alignment: .leading) {
                Capsule().fill(.quaternary).frame(height: 4)
                Circle()
                    .fill(.primary)
                    .frame(width: 8, height: 8)
                    .offset(x: position - 4)
            }
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        if dragging == nil { store.beginInteraction(param.name) }
                        let v = g.value(at: drag.location.x)
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
        .frame(width: Self.size, height: 12)
    }
}
