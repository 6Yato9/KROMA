import CoreGraphics
import SwiftUI

/// One lattice, drawn over the slice of colour it warps.
///
/// The lattice is drawn *displaced*: a vertex sits where its own offset has put
/// it, so the web itself shows the shape of the edit. A grid that stayed put
/// and showed the displacement some other way would be a table of numbers with
/// lines between them.
///
/// The in-flight lattice is held here and the snapshot is not refreshed
/// mid-drag, for the reason `FloatRow` and `CurveEditor` do the same.
public struct WarpEditor: View {
    let param: Param
    let axes: WarpAxes
    let row: UInt64
    let value: WarpValue
    let isActive: Bool
    let store: SessionStore

    /// The vertex being dragged, decided once when the drag starts. Picking the
    /// nearest one every frame instead would hand the drag to a neighbour the
    /// moment it passed under the pointer.
    @State private var held: (col: Int, row: Int)?
    @State private var live: WarpValue?

    public init(
        param: Param, axes: WarpAxes, row: UInt64, value: WarpValue,
        isActive: Bool, store: SessionStore
    ) {
        self.param = param
        self.axes = axes
        self.row = row
        self.value = value
        self.isActive = isActive
        self.store = store
    }

    private var warp: WarpValue { live ?? value }

    public var body: some View {
        GeometryReader { geo in
            plot(in: geo.size)
        }
        .aspectRatio(1, contentMode: .fit)
        .frame(maxWidth: 320)
        .opacity(isActive ? 1 : 0.4)
        .disabled(!isActive)
    }

    /// Split out of `body` so the type checker sees one small expression at a
    /// time rather than a ZStack, a gesture and four geometry lets at once.
    private func plot(in size: CGSize) -> some View {
        let side = min(size.width, size.height)
        let rect = CGRect(
            x: (size.width - side) / 2,
            y: (size.height - side) / 2,
            width: side, height: side
        )
        let g = WarpGeometry(warp: warp, axes: axes, rect: rect)
        return ZStack {
            background(rect)
            lattice(g)
            vertices(g)
        }
        .contentShape(Rectangle())
        .gesture(drag(g))
    }

    // ---- the space itself ------------------------------------------------

    /// The slice of colour the lattice sits over.
    ///
    /// Resolve, and the Windows shell, composite this with a haze showing where
    /// this photograph's own colours actually fall. That needs scope data,
    /// which has no C ABI yet — so what is drawn here is the space and not the
    /// picture. A lattice over a black square would say nothing at all about
    /// which colours are being moved, which is why the space is drawn even
    /// though the haze cannot be.
    @ViewBuilder
    private func background(_ rect: CGRect) -> some View {
        switch axes {
        case .hueSat:
            Circle()
                .fill(hues)
                .overlay(
                    // Saturation grows outward, so the middle washes out.
                    RadialGradient(
                        colors: [Color(white: 0.5), Color(white: 0.5).opacity(0)],
                        center: .center, startRadius: 0,
                        endRadius: rect.width * 0.45
                    )
                    .clipShape(Circle())
                )
                .frame(width: rect.width * 0.9, height: rect.height * 0.9)
                .position(x: rect.midX, y: rect.midY)
        case .chromaLuma:
            // Chroma across, luma up.
            LinearGradient(
                colors: [Color(white: 0.5), Color(hue: 0.05, saturation: 1, brightness: 1)],
                startPoint: .leading, endPoint: .trailing
            )
            .overlay(
                LinearGradient(
                    colors: [.white.opacity(0.85), .clear, .black.opacity(0.85)],
                    startPoint: .top, endPoint: .bottom
                )
            )
            .frame(width: rect.width, height: rect.height)
            .position(x: rect.midX, y: rect.midY)
        }
    }

    private var hues: AngularGradient {
        let wheel: [Color] = stride(from: 0.0, through: 1.0, by: 1.0 / 12).map {
            Color(hue: $0, saturation: 1, brightness: 1)
        }
        return AngularGradient(colors: wheel, center: .center)
    }

    // ---- the web ---------------------------------------------------------

    private func lattice(_ g: WarpGeometry) -> some View {
        Path { p in
            // Along the first axis, closing the ring when it is one.
            for r in 0..<warp.rows {
                let last = axes.wraps ? warp.cols : warp.cols - 1
                for c in 0..<max(last, 0) {
                    let next = (c + 1) % warp.cols
                    p.move(to: g.toScreen(g.displaced(col: c, row: r)))
                    p.addLine(to: g.toScreen(g.displaced(col: next, row: r)))
                }
            }
            // And along the second, which never closes.
            for c in 0..<warp.cols {
                for r in 0..<max(warp.rows - 1, 0) {
                    p.move(to: g.toScreen(g.displaced(col: c, row: r)))
                    p.addLine(to: g.toScreen(g.displaced(col: c, row: r + 1)))
                }
            }
        }
        .stroke(.white.opacity(0.6), lineWidth: 1)
    }

    private func vertices(_ g: WarpGeometry) -> some View {
        ForEach(0..<(warp.cols * warp.rows), id: \.self) { i in
            let c = i % warp.cols
            let r = i / warp.cols
            let moved = warp.at(col: c, row: r) != .zero
            Circle()
                .fill(moved ? Color.accentColor : Color.white.opacity(0.85))
                .frame(width: moved ? 6.4 : 4.8, height: moved ? 6.4 : 4.8)
                .position(g.toScreen(g.displaced(col: c, row: r)))
        }
    }

    // ---- dragging --------------------------------------------------------

    private func drag(_ g: WarpGeometry) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { gesture in
                if held == nil {
                    guard let hit = g.nearest(to: gesture.location) else { return }
                    store.beginInteraction(param.name)
                    held = hit
                    live = value
                }
                guard let hit = held else { return }
                let offset = g.offset(draggingCol: hit.col, row: hit.row, to: gesture.location)
                live = warp.replacing(col: hit.col, row: hit.row, with: offset)
                store.setWarpVertex(
                    row: row, key: param.key, col: hit.col, vertexRow: hit.row, offset: offset
                )
            }
            .onEnded { _ in
                if held != nil { store.endInteraction() }
                held = nil
                live = nil
            }
    }
}
