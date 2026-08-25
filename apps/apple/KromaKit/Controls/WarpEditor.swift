import CoreGraphics
import SwiftUI

// -----------------------------------------------------------------------------
// The photograph's own colours, behind the plot that moves them
// -----------------------------------------------------------------------------

/// The haze of where this frame's colours actually are, over one of the
/// warper's plots.
///
/// A grid you can drag, over a plot of the whole colour space, tells you
/// nothing about the photograph in front of you — you would be aiming at where
/// greens are *in general*. This is the difference between grading and
/// guessing, and it is the same argument `CurveBackdropView` makes for the
/// counts behind a curve.
///
/// All of it is static and takes only counts, so the arithmetic that decides
/// *where* a colour is drawn is testable without a display — which matters
/// more here than anywhere else in this file, because a cloud that is
/// mirrored or a quarter-turn out still looks entirely plausible.
public enum WarperCloud {

    /// Which plot a cloud is being drawn on.
    ///
    /// `Scopes.WarperClouds` carries three grids rather than one because the
    /// three views are three different projections, and each is read back *in
    /// its own view's terms*. Naming the plot rather than passing a mapping
    /// keeps the three conversions in one place, next to the binning they have
    /// to agree with.
    public enum Plot: Sendable, Hashable {
        /// CIE xy over `PinGeometry`'s range, which is what `PinsEditor` draws
        /// and what `pe_core::pins::plot_fraction` bins.
        case chromaticity
        /// Hue around, saturation out, on the disc `WarpGeometry` lays out.
        case hueSat
        /// Chroma across, luma up.
        case chromaLuma
    }

    /// How many texels across a cloud is built.
    ///
    /// Built at a fixed size and scaled to whatever the panel gives it rather
    /// than rebuilt on every resize: the haze is smooth by construction, so a
    /// linear filter loses nothing you can see. One image rather than 16,384
    /// rectangles, for the reason `ScopeImage` states at length.
    public static let texels = 256

    /// How much of the plot a full-scale cell reaches. The Windows shell's
    /// `plot_image`: well under full, because the plot is a map and the
    /// photograph's colours are what you came to look at.
    static let ceiling = 0.85

    // ---- the mapping -----------------------------------------------------

    /// Where a point on a plot sits in the grid measured *for that plot*, or
    /// nil where the plot has no colours at all.
    ///
    /// `u` runs across and `v` runs **up**, both fractions of the plot's own
    /// square — the same convention every plot here draws in.
    ///
    /// The three are not the same mapping, and this is the whole of Task 4:
    ///
    /// - **chromaticity** is binned with `pe_core::pins::plot_fraction`, the
    ///   same mapping `PinGeometry` uses, so the cloud and the pins agree.
    ///   Plot fraction *is* grid fraction.
    /// - **chromaLuma** is binned 0…1 on each axis, matching its plot directly.
    /// - **hueSat** is stored in the **square containing the unit disc** —
    ///   a colour at hue *h*, saturation *s* lands at
    ///   `((s·cos h + 1)/2, (s·sin h + 1)/2)`. That is *not* `WarpGeometry`'s
    ///   polar mapping, and reading it as though it were would put the cloud on
    ///   the right plot in the wrong place. The conversion goes through the
    ///   square: a point on the plot is an offset from the middle, divided by
    ///   the radius full saturation reaches, and that offset is already the
    ///   `(s·cos h, s·sin h)` the grid stores.
    public static func gridFraction(
        _ plot: Plot, u: Double, v: Double
    ) -> (u: Double, v: Double)? {
        switch plot {
        case .chromaticity, .chromaLuma:
            return (u, v)
        case .hueSat:
            let radius = Double(WarpGeometry.radiusFraction)
            let x = (u - 0.5) / radius
            let y = (v - 0.5) / radius
            // Outside the disc there are no colours, so there is nothing to
            // say there. The blur happily spreads counts past the boundary,
            // which drew a smear over a region that has no colours in it by
            // definition.
            guard x * x + y * y <= 1 else { return nil }
            return ((x + 1) / 2, (y + 1) / 2)
        }
    }

    // ---- the counts ------------------------------------------------------

    /// Spread each cell's count over its neighbours.
    ///
    /// Two separable passes with a `[1, 4, 6, 4, 1] / 16` kernel — a five-wide
    /// binomial, near enough a Gaussian at this size and two multiply-adds a
    /// cell instead of twenty-five.
    ///
    /// Blurred *before* it is drawn, because a frame's colours are a sample of
    /// a continuous distribution and at 128² most cells hold nothing or one:
    /// reading between them bilinearly still shows the lattice, because the
    /// lattice is genuinely what the counts look like.
    public static func blurred(_ plane: Scopes.Plane) -> [Double] {
        let k: [Double] = [1, 4, 6, 4, 1]
        let (w, h) = (plane.width, plane.height)
        guard w > 0, h > 0, plane.counts.count == w * h else { return [] }
        var across = [Double](repeating: 0, count: w * h)
        for y in 0..<h {
            let base = y * w
            for x in 0..<w {
                var total = 0.0
                for (i, weight) in k.enumerated() {
                    total += Double(plane.counts[base + min(max(x + i - 2, 0), w - 1)]) * weight
                }
                across[base + x] = total / 16
            }
        }
        var out = [Double](repeating: 0, count: w * h)
        for y in 0..<h {
            for x in 0..<w {
                var total = 0.0
                for (i, weight) in k.enumerated() {
                    total += across[min(max(y + i - 2, 0), h - 1) * w + x] * weight
                }
                out[y * w + x] = total / 16
            }
        }
        return out
    }

    /// A count grid read at a point, bilinearly.
    ///
    /// Bilinear because the grid is coarser than the image it is drawn into,
    /// and the complaint the Windows shell recorded about its own first attempt
    /// was that you could see the cells.
    ///
    /// `v` runs **up** and the grid stores it **down** — `bump` puts v = 1 in
    /// row zero. Getting this the wrong way round flips a cloud vertically and
    /// leaves it looking entirely plausible.
    public static func sample(
        _ grid: [Double], width: Int, height: Int, u: Double, v: Double
    ) -> Double {
        guard width > 0, height > 0, grid.count == width * height else { return 0 }
        let fx = min(max(u * Double(width) - 0.5, 0), Double(width - 1))
        let fy = min(max((1 - v) * Double(height) - 0.5, 0), Double(height - 1))
        let x0 = Int(fx), y0 = Int(fy)
        let x1 = min(x0 + 1, width - 1), y1 = min(y0 + 1, height - 1)
        let tx = fx - Double(x0), ty = fy - Double(y0)
        let at = { (x: Int, y: Int) in grid[y * width + x] }
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx
        return top + (bottom - top) * ty
    }

    /// How dense a reading becomes, as a fraction of full brightness.
    ///
    /// A fourth root against the blurred peak. A photograph's colours are
    /// wildly unevenly distributed — a sky is thousands of pixels in a handful
    /// of cells and a red jacket is a hundred over dozens. On a linear scale
    /// the jacket is invisible, and seeing the jacket is the entire point.
    public static func haze(_ reading: Double, peak: Double) -> Double {
        guard peak > 0, reading > 0 else { return 0 }
        return min(max(pow(reading / peak, 0.25), 0), 1) * ceiling
    }

    // ---- the bytes -------------------------------------------------------

    /// The haze for one plot, as an image, or nil when there is nothing to say.
    ///
    /// White and premultiplied, with alpha equal to the haze: drawn with
    /// `.plusLighter` that *adds* to the space beneath rather than tinting it,
    /// so a dense cloud over a green still reads as a green with a lot of
    /// pixels in it — and an empty cell adds nothing at all, rather than laying
    /// an opaque black over whatever the plot is drawn on.
    public static func image(_ plane: Scopes.Plane, plot: Plot) -> CGImage? {
        let grid = blurred(plane)
        guard let peak = grid.max(), peak > 0 else { return nil }
        let n = texels
        var bytes = [UInt8](repeating: 0, count: n * n * 4)
        for row in 0..<n {
            // Texel centres, and v measured upwards to match every plot here.
            let v = 1 - (Double(row) + 0.5) / Double(n)
            for col in 0..<n {
                let u = (Double(col) + 0.5) / Double(n)
                guard let g = gridFraction(plot, u: u, v: v) else { continue }
                let reading = sample(
                    grid, width: plane.width, height: plane.height, u: g.u, v: g.v)
                let byte = UInt8(min(max(haze(reading, peak: peak) * 255, 0), 255))
                guard byte > 0 else { continue }
                let i = (row * n + col) * 4
                bytes[i] = byte
                bytes[i + 1] = byte
                bytes[i + 2] = byte
                bytes[i + 3] = byte
            }
        }
        guard let provider = CGDataProvider(data: Data(bytes) as CFData) else { return nil }
        return CGImage(
            width: n, height: n,
            bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: n * 4,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent
        )
    }
}

extension Scopes.WarperClouds {
    /// The grid measured for one plot.
    ///
    /// The pairing lives here rather than at each call site, so a plot cannot
    /// be drawn with another plot's cloud. That would be a picture of a real
    /// distribution in a place it was never measured — the most convincing kind
    /// of wrong there is, since every cell of it is a colour the photograph
    /// genuinely has.
    public func plane(for plot: WarperCloud.Plot) -> Scopes.Plane {
        switch plot {
        case .chromaticity: return chromaticity
        case .hueSat: return hueSat
        case .chromaLuma: return chromaLuma
        }
    }
}

/// The last cloud built, kept across body evaluations.
///
/// A reference type written to from `body`, which is safe here for the reason
/// `ScopeImageMemo` states: it is a memo and not state, so nothing about what
/// the view looks like depends on whether it was hit or missed. Its own type
/// rather than `ScopeImageMemo` because a cloud's alpha is its density and
/// `ScopeImage.Raster` writes 255 into every pixel it touches — which over a
/// plot that is not itself opaque would be a black square with a haze on it.
final class WarperCloudMemo {
    private var key: AnyHashable?
    private var image: CGImage?

    func image(for key: AnyHashable, build: () -> CGImage?) -> CGImage? {
        guard self.key != key || image == nil else { return image }
        self.key = key
        image = build()
        return image
    }
}

/// One plot's haze, rebuilt only when the measurement moves.
///
/// The key is what stops sixty-five thousand texels of `pow` being rebuilt on
/// every body evaluation — which, with a vertex under the user's thumb, is
/// sixty times a second.
struct WarperCloudView: View {
    /// All three, and which one this is — rather than the one grid, so the
    /// caller cannot hand a plot the wrong cloud.
    let clouds: Scopes.WarperClouds
    let plot: WarperCloud.Plot
    let generation: UInt64

    @State private var memo = WarperCloudMemo()

    private struct Key: Hashable {
        let generation: UInt64
        let plot: WarperCloud.Plot
    }

    var body: some View {
        if let image = memo.image(
            for: Key(generation: generation, plot: plot),
            build: { WarperCloud.image(clouds.plane(for: plot), plot: plot) }
        ) {
            Image(decorative: image, scale: 1)
                .resizable()
                .interpolation(.high)
                // Added, not painted over: the haze brightens the plot's own
                // colours where the photograph has them.
                .blendMode(.plusLighter)
                .allowsHitTesting(false)
        }
    }
}

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
            cloud(rect)
            lattice(g)
            vertices(g)
        }
        .contentShape(Rectangle())
        .gesture(drag(g))
    }

    // ---- the space itself ------------------------------------------------

    /// The slice of colour the lattice sits over.
    ///
    /// The space itself. Where this photograph's colours actually fall goes
    /// over it in `cloud`, added rather than painted on — the two together are
    /// what Resolve and the Windows shell composite, and a lattice over either
    /// one alone says only half of what is being moved.
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

    /// Where this photograph's own colours are, over the space they sit in.
    ///
    /// Nil scopes draw nothing, and in particular do not *ask* for a
    /// measurement: the editor is not what decides when to measure, and a view
    /// that started a full render plus readback from its own body would do it
    /// on every layout pass. The same rule `CurveBackdropView` follows.
    @ViewBuilder
    private func cloud(_ rect: CGRect) -> some View {
        if let scopes = store.scopes {
            WarperCloudView(
                clouds: scopes.warper,
                plot: axes == .hueSat ? .hueSat : .chromaLuma,
                generation: scopes.generation
            )
            .frame(width: rect.width, height: rect.height)
            .position(x: rect.midX, y: rect.midY)
        }
    }

    /// The hue wheel, running the way the lattice and the cloud run.
    ///
    /// **Reversed**, and that is the point. An `AngularGradient` sweeps
    /// *clockwise* from three o'clock, because view y grows downwards; hue in
    /// this control runs anticlockwise — `toScreen` is `midY - r·sin(a)`, and
    /// the engine bins a colour at `(sat·cos h, sat·sin h)` with v measured
    /// upwards. Painted in gradient order the wheel agreed with the lattice at
    /// red and cyan and was the complementary hue everywhere else: a vertex
    /// dragged into what looked like the greens moved the violets, and the
    /// haze brightened the opposite side of the wheel from the colours it was
    /// measured from.
    private var hues: AngularGradient {
        let wheel: [Color] = stride(from: 0.0, through: 1.0, by: 1.0 / 12).map {
            Color(hue: 1 - $0, saturation: 1, brightness: 1)
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
