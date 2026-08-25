import SwiftUI

/// Resolve's inspector row: a right-aligned label, a thin track with a pointer
/// handle, a boxed number, and a reset arrow.
///
/// The column widths are Resolve's own, read off the Windows shell's
/// `resolve.rs`. Getting the row right once is most of what makes a panel look
/// like Resolve; doing it by hand at each call site is how the columns end up
/// not lining up, which is immediately obvious in a panel of thirty controls.
enum RowMetrics {
    static let label: CGFloat = 112
    static let value: CGFloat = 58
    static let reset: CGFloat = 18
    static let gap: CGFloat = 6
    static let height: CGFloat = 22

    /// The narrowest a track may be drawn.
    ///
    /// Not a look but a floor on usefulness: a slider this short gives a
    /// hundredth of its range to about half a point of travel, which is not a
    /// control anybody can aim. Below this the panel should get wider, not the
    /// track shorter.
    static let track: CGFloat = 72

    /// What one row costs, side to side.
    ///
    /// Spelled out because the label, the readout and the reset arrow are all
    /// fixed widths, and a panel narrower than their sum plus a usable track
    /// cannot draw a row — the fixed parts overflow and clip at *both* ends,
    /// which reads as "Temperature" losing its first six letters rather than
    /// as a layout that did not fit.
    static let minimumRow: CGFloat = label + gap + track + gap + value + gap + reset

    /// The inspector's own inset, either side.
    static let inset: CGFloat = 8

    /// The narrowest the inspector may be. `ContentView` reads this rather
    /// than carrying its own number, so the two cannot disagree.
    static let minimumPanel: CGFloat = minimumRow + inset * 2

    /// Everything in a row that is not the track.
    static let fixed: CGFloat = minimumRow - track

    /// The track's share of a row this wide.
    ///
    /// The track gives way last, not first: the label, the readout and the
    /// reset arrow are fixed columns, so anything left over is the track's,
    /// down to the floor above.
    static func trackWidth(inRowOf width: CGFloat) -> CGFloat {
        Swift.max(track, width - fixed)
    }
}

/// The pointer that marks the value.
///
/// A house shape with its point up, not a circle. A circle marks a position; a
/// point marks a *place on a scale*, which is what a slider has. On a coloured
/// track that difference is the whole game — a disc covers the part of the
/// gradient you are trying to read, and its widest part sits exactly where you
/// want to see the colour underneath. This shape is at its narrowest at the
/// top, where it points, and only reaches its full width below the middle of
/// the track.
struct Pointer: Shape {
    /// Where the roof meets the walls, as a fraction of the height. The
    /// Windows shell's pointer runs from 7 points above the track's centre to
    /// 5.5 below it, with the shoulders 1.5 above: 5.5 of 12.5.
    static let shoulder: CGFloat = 5.5 / 12.5

    func path(in rect: CGRect) -> Path {
        var path = Path()
        let shoulder = rect.minY + rect.height * Self.shoulder
        path.move(to: CGPoint(x: rect.midX, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.maxX, y: shoulder))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX, y: shoulder))
        path.closeSubpath()
        return path
    }
}

/// The look of an inspector row — label, track, readout, reset arrow — without
/// any opinion about where the number lives.
///
/// `FloatRow` drives a registry parameter through the store. A pin's five
/// controls are floats *inside* a parameter and cannot use that path, but they
/// have to look identical: a panel where one row is drawn differently from the
/// thirty above it reads as a bug.
///
/// The in-flight value is held here rather than read back from wherever the
/// number lives, so the document is not diffed sixty times a second while a
/// finger is down. Bracketing the drag into one undo step is the *caller's*
/// business, because only it knows what the drag is of — hence `onBegin` and
/// `onEnd` rather than a store reference.
///
/// Every colour here comes from ``Palette``, and nothing from SwiftUI's own
/// semantic styles. A `.tint` fill is the system accent, which is whatever the
/// user set it to in System Settings; a panel of thirty of those shares a
/// palette with nothing else in the application.
public struct ScalarRow: View {
    let name: String
    let unit: String
    let value: Float
    let bounds: Bounds
    /// What the track is measuring, drawn. `.plain` is the grey bar — which is
    /// what the pin controls get, since they have no registry key to look a
    /// ramp up by.
    let ramp: Ramp
    let isActive: Bool
    /// Called on every frame of a drag, and once each for the reset arrow and
    /// a typed-in number — both discrete changes and their own undo steps, so
    /// they arrive outside any `onBegin`/`onEnd` pair.
    let onChange: (Float) -> Void
    let onBegin: () -> Void
    let onEnd: () -> Void

    @State private var dragging: Float?
    @State private var hovering = false
    /// Where a drag on the *box* started from. The box is a relative control —
    /// it moves the value by a fraction of how far the pointer went — so it
    /// needs the value it began at, which the track does not.
    @State private var boxAnchor: Float?
    @State private var typing = false
    @State private var typed = ""
    @FocusState private var focused: Bool

    public init(
        name: String, unit: String, value: Float, bounds: Bounds, ramp: Ramp = .plain,
        isActive: Bool,
        onChange: @escaping (Float) -> Void,
        onBegin: @escaping () -> Void,
        onEnd: @escaping () -> Void
    ) {
        self.name = name
        self.unit = unit
        self.value = value
        self.bounds = bounds
        self.ramp = ramp
        self.isActive = isActive
        self.onChange = onChange
        self.onBegin = onBegin
        self.onEnd = onEnd
    }

    private var shown: Float { dragging ?? value }

    // ---- the numbers the drawing is built from ---------------------------

    /// Half the pointer's width, and the inset at each end of the track.
    ///
    /// The value's span is measured across the track *minus* this at both
    /// ends, so the pointer never hangs off the bar and clicking where the
    /// pointer looks like it should go puts it there. Without the inset the
    /// last few points at each end cannot be reached by a click.
    static let handleHalfWidth: CGFloat = 5
    static let handleWidth: CGFloat = handleHalfWidth * 2
    static let handleHeight: CGFloat = 12.5
    /// The pointer straddles the track's centre 7 above and 5.5 below, so its
    /// own middle sits three quarters of a point high.
    static let handleRise: CGFloat = -0.75
    /// The plain bar. The ramped one is a half-point taller, which is what the
    /// Windows shell draws — a gradient needs the extra row to read as a
    /// gradient rather than as a line.
    static let barHeight: CGFloat = 4
    static let rampHeight: CGFloat = 5
    static let neutralMarkHeight: CGFloat = 9
    /// Shorter than the row it sits in. A field that fills its row reads as a
    /// button, and thirty of them stacked up is a wall of boxes rather than a
    /// column of numbers.
    static let boxHeight: CGFloat = 17
    /// What a row that cannot do anything is multiplied by.
    ///
    /// SwiftUI's `.disabled` will not do it: it adjusts the *semantic* styles,
    /// which is no help to anything painted with a colour of its own — and
    /// everything here is.
    static let dimmed: Double = 0.42
    /// How much finer dragging the number is than dragging the track.
    ///
    /// The track crosses its whole range in the width of the panel; the box
    /// takes four times as far. That ratio is the point of having both — the
    /// track is for finding roughly the right value and the box is for
    /// settling on one, and a box that moved at the same rate would just be a
    /// second slider.
    static let fine: Float = 4

    // ---- where a value sits, and what a drag makes of it ------------------

    /// The track's own arithmetic, over the span the pointer can actually
    /// reach. Reusing ``SliderGeometry`` rather than restating it keeps this
    /// row on the same division-by-zero guards everything else uses.
    static func trackGeometry(bounds: Bounds, over width: CGFloat) -> SliderGeometry {
        SliderGeometry(bounds: bounds, width: Swift.max(0, width - handleWidth))
    }

    /// Where a value sits, in points from the track's left edge.
    static func trackPosition(of value: Float, bounds: Bounds, over width: CGFloat) -> CGFloat {
        handleHalfWidth + trackGeometry(bounds: bounds, over: width).position(of: value)
    }

    /// What a point on the track means.
    static func valueOnTrack(bounds: Bounds, at x: CGFloat, over width: CGFloat) -> Float {
        trackGeometry(bounds: bounds, over: width).value(at: x - handleHalfWidth)
    }

    /// The value a drag of `by` points along the track arrives at — the same
    /// arithmetic the track's own gesture runs, said where a test can call it.
    static func valueDraggingTrack(
        bounds: Bounds, from: Float, by: CGFloat, over: CGFloat
    ) -> Float {
        valueOnTrack(
            bounds: bounds,
            at: trackPosition(of: from, bounds: bounds, over: over) + by,
            over: over)
    }

    /// The same drag, on the box. A quarter of the rate, derived from the
    /// track's own width so the ratio holds at any panel size rather than
    /// being tuned for one.
    static func valueDraggingBox(
        bounds: Bounds, from: Float, by: CGFloat, over: CGFloat
    ) -> Float {
        let span = bounds.max - bounds.min
        let width = Float(Swift.max(over, RowMetrics.track))
        return clamp(from + Float(by) * span / (width * fine), to: bounds)
    }

    static func clamp(_ value: Float, to bounds: Bounds) -> Float {
        Swift.min(Swift.max(value, bounds.min), bounds.max)
    }

    /// Where neutral sits along the track, when it is somewhere you could miss
    /// it.
    ///
    /// `nil` when the parameter's neutral is at either end — an exposure
    /// slider that starts at zero needs no mark, because the left end already
    /// is one.
    static func neutralMark(bounds: Bounds) -> Float? {
        let span = bounds.max - bounds.min
        guard abs(span) > 1e-9 else { return nil }
        let t = (bounds.neutral - bounds.min) / span
        return (t >= 0.04 && t < 0.96) ? t : nil
    }

    // ---- the row ---------------------------------------------------------

    public var body: some View {
        GeometryReader { geo in
            row(width: geo.size.width)
        }
        .frame(height: RowMetrics.height)
        .frame(minWidth: RowMetrics.minimumRow)
    }

    private func row(width: CGFloat) -> some View {
        let track = RowMetrics.trackWidth(inRowOf: width)
        return HStack(spacing: RowMetrics.gap) {
            labelText
            trackView(width: track)
            valueBox(over: track)
            resetButton
        }
        .frame(width: width, height: RowMetrics.height, alignment: .leading)
        .opacity(isActive ? 1 : ScalarRow.dimmed)
        .disabled(!isActive)
    }

    private var labelText: some View {
        Text(name)
            .font(.system(size: 11.5))
            .foregroundStyle(Palette.label.color)
            .lineLimit(1)
            .frame(width: RowMetrics.label, alignment: .trailing)
    }

    private var resetButton: some View {
        Button {
            onChange(bounds.neutral)
        } label: {
            Image(systemName: "arrow.uturn.backward")
                .imageScale(.small)
                .foregroundStyle(Palette.icon.color)
        }
        .buttonStyle(.borderless)
        .frame(width: RowMetrics.reset)
        .help("Back to \(format(bounds.neutral))")
    }

    // ---- the readout -----------------------------------------------------

    private var readout: String {
        unit.isEmpty ? format(shown) : "\(format(shown)) \(unit)"
    }

    private func format(_ v: Float) -> String {
        // A temperature in kelvin has no useful fraction; an exposure in stops
        // is nothing but fraction.
        abs(bounds.max - bounds.min) > 100
            ? String(format: "%.0f", v)
            : String(format: "%.2f", v)
    }

    // ---- the track -------------------------------------------------------

    private var hot: Bool { hovering || dragging != nil }

    private func trackView(width: CGFloat) -> some View {
        ZStack(alignment: .leading) {
            bar(width: width)
            neutralTick(width: width)
            pointer(width: width)
        }
        .frame(width: width, height: RowMetrics.height)
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .gesture(trackDrag(width: width))
    }

    /// The bar, and how far from neutral the value has been pushed.
    ///
    /// A ramped track gets the gradient and **no fill**: the gradient is
    /// already showing the axis, and a bar of flat grey over it would hide the
    /// part being pointed at.
    @ViewBuilder
    private func bar(width: CGFloat) -> some View {
        if ramp.isPlain {
            RoundedRectangle(cornerRadius: 2)
                .fill(Palette.track.color)
                .frame(width: width, height: ScalarRow.barHeight)
            fillBar(width: width)
        } else {
            ScalarRow.gradient(ramp)
                .frame(width: width, height: ScalarRow.rampHeight)
                .clipShape(RoundedRectangle(cornerRadius: 2))
        }
    }

    /// Filled from **neutral** to the value, not from the left end. On a
    /// bipolar control the fill growing out of the middle is the only drawing
    /// that gives you the sign at a glance.
    @ViewBuilder
    private func fillBar(width: CGFloat) -> some View {
        let filled = ScalarRow.trackGeometry(bounds: bounds, over: width).fill(for: shown)
        if filled.width > 0.5 {
            RoundedRectangle(cornerRadius: 2)
                .fill(Palette.trackFill.color)
                .frame(width: filled.width, height: ScalarRow.barHeight)
                .offset(x: ScalarRow.handleHalfWidth + filled.origin)
        }
    }

    /// The tick where the parameter does nothing.
    ///
    /// "Put it back where it was" is the most common thing anyone wants from a
    /// slider they have pushed too far, and a mark is cheaper to read than the
    /// number.
    @ViewBuilder
    private func neutralTick(width: CGFloat) -> some View {
        if let t = ScalarRow.neutralMark(bounds: bounds) {
            let span = Swift.max(0, width - ScalarRow.handleWidth)
            Rectangle()
                .fill(Palette.handleEdge.color)
                .frame(width: 1, height: ScalarRow.neutralMarkHeight)
                .offset(x: ScalarRow.handleHalfWidth + CGFloat(t) * span - 0.5)
        }
    }

    /// The pointer, outlined.
    ///
    /// The outline is not decoration: the fill is a light grey and against the
    /// pale end of a temperature or luma ramp it would otherwise vanish.
    private func pointer(width: CGFloat) -> some View {
        let x = ScalarRow.trackPosition(of: shown, bounds: bounds, over: width)
        return Pointer()
            .fill(hot ? Palette.handleHot.color : Palette.handle.color)
            .overlay(Pointer().stroke(Palette.handleEdge.color, lineWidth: 1))
            .frame(width: ScalarRow.handleWidth, height: ScalarRow.handleHeight)
            .offset(x: x - ScalarRow.handleHalfWidth, y: ScalarRow.handleRise)
    }

    /// A ramp, as a gradient.
    ///
    /// Sampled from ``Ramp`` at a fixed number of stops rather than described
    /// twice: that type is checked against the engine's own colours at every
    /// step, so a stop taken from it is a colour the fixture has already
    /// agreed to. Twenty-four is past the point where more of them change the
    /// picture — it is what the Windows shell uses for the same reason.
    static func gradient(_ ramp: Ramp) -> LinearGradient {
        let steps = 24
        let stops = (0...steps).map { i -> Gradient.Stop in
            let t = Double(i) / Double(steps)
            return Gradient.Stop(color: ramp.at(t).color, location: t)
        }
        return LinearGradient(
            gradient: Gradient(stops: stops), startPoint: .leading, endPoint: .trailing)
    }

    private func trackDrag(width: CGFloat) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { drag in
                if dragging == nil { onBegin() }
                let v = ScalarRow.valueOnTrack(bounds: bounds, at: drag.location.x, over: width)
                dragging = v
                onChange(v)
            }
            .onEnded { _ in
                onEnd()
                dragging = nil
            }
    }

    // ---- the box ---------------------------------------------------------

    /// The boxed number: a second control, not a second readout.
    ///
    /// Drag it and the value moves a quarter as fast as it does on the track;
    /// double-click it and type one exactly.
    private func valueBox(over track: CGFloat) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: 2)
                .fill(Palette.boxFill.color)
            RoundedRectangle(cornerRadius: 2)
                .strokeBorder(Palette.boxEdge.color, lineWidth: 1)
            boxContent(over: track)
        }
        .frame(width: RowMetrics.value, height: ScalarRow.boxHeight)
    }

    @ViewBuilder
    private func boxContent(over track: CGFloat) -> some View {
        if typing {
            TextField("", text: $typed)
                .textFieldStyle(.plain)
                .multilineTextAlignment(.trailing)
                .font(.system(size: 11))
                .monospacedDigit()
                .foregroundStyle(Palette.title.color)
                .focused($focused)
                .onSubmit { commitTyped() }
                .padding(.horizontal, 4)
        } else {
            Text(readout)
                .font(.system(size: 11))
                .monospacedDigit()
                .foregroundStyle(Palette.label.color)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .trailing)
                .padding(.horizontal, 4)
                .contentShape(Rectangle())
                .gesture(boxDrag(over: track))
                .onTapGesture(count: 2) { beginTyping() }
        }
    }

    private func boxDrag(over track: CGFloat) -> some Gesture {
        DragGesture(minimumDistance: 2)
            .onChanged { drag in
                if boxAnchor == nil {
                    boxAnchor = shown
                    onBegin()
                }
                let v = ScalarRow.valueDraggingBox(
                    bounds: bounds, from: boxAnchor ?? shown,
                    by: drag.translation.width, over: track)
                dragging = v
                onChange(v)
            }
            .onEnded { _ in
                onEnd()
                dragging = nil
                boxAnchor = nil
            }
    }

    private func beginTyping() {
        typed = format(shown)
        typing = true
        focused = true
    }

    /// A typed number is a discrete change, so it goes in outside the
    /// `onBegin`/`onEnd` bracket — one undo step, like the reset arrow.
    private func commitTyped() {
        guard typing else { return }
        typing = false
        focused = false
        if let v = Float(typed.trimmingCharacters(in: .whitespaces)) {
            onChange(ScalarRow.clamp(v, to: bounds))
        }
    }
}

/// One float parameter, as a draggable track.
///
/// The drag is bracketed so it becomes one undo step: `beginInteraction` on
/// the way down, one engine call per frame, `endInteraction` on the way up.
///
/// Nothing but the wiring: the drawing is `ScalarRow`'s, so a pin's controls
/// and a registry parameter's cannot drift apart.
public struct FloatRow: View {
    let effectName: String
    let param: Param
    let bounds: Bounds
    let row: UInt64
    let value: Float
    let isActive: Bool
    let store: SessionStore

    public init(
        effectName: String, param: Param, bounds: Bounds, row: UInt64,
        value: Float, isActive: Bool, store: SessionStore
    ) {
        self.effectName = effectName
        self.param = param
        self.bounds = bounds
        self.row = row
        self.value = value
        self.isActive = isActive
        self.store = store
    }

    public var body: some View {
        ScalarRow(
            name: param.name, unit: param.unit, value: value, bounds: bounds,
            // Keyed off the parameter rather than the panel, because the same
            // parameter appears in several: Temp. Shift inside Film Damage is
            // the same axis as Temperature in Basic.
            ramp: Ramp.for(effect: effectName, key: param.key),
            isActive: isActive,
            onChange: { store.setFloat(row: row, key: param.key, value: $0) },
            onBegin: { store.beginInteraction(param.name) },
            onEnd: { store.endInteraction() }
        )
    }
}
