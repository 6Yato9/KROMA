import Foundation

/// What a slider's track is filled with, and which parameter gets which.
///
/// A mirror of `crates/pe-theme/src/ramp.rs`, which is a thing worth being
/// uncomfortable about — so every ramp is checked against that side at every
/// one of seventeen steps, byte for byte, from a fixture it generates. See
/// `PaletteTests.testEveryRampPaintsWhatTheEngineWouldPaint`.
///
/// A grey track says nothing about what it does; a track that runs blue to
/// yellow says *temperature* before the label is read. The rule for adding one:
/// the gradient must show the parameter's own axis. Exposure gets no ramp,
/// because a black-to-white track under a control that moves the whole picture
/// would be decoration pretending to be information.
///
/// The arithmetic below is all `Float`, deliberately, and not `Double` as the
/// rest of this shell's geometry is. The engine's is `f32`, the assertion is
/// exact colour equality, and a ramp computed at double precision lands on the
/// other side of a rounding boundary often enough to fail it.
public enum Ramp: Equatable, Sendable {
    case plain
    /// Cool to warm, through neutral.
    case temp
    /// Green to magenta, through neutral — the other white-balance axis.
    case tint
    /// The whole hue circle.
    case hue
    /// A window of the hue circle, centred on one band's own colour, so the
    /// mixer's Red row shows what red actually shifts towards.
    case hueAround(Float)
    /// Grey to that band's colour.
    case sat(Rgb8)
    /// Grey to increasingly colourful, across the spectrum. What a master
    /// saturation control does, drawn.
    case chroma
    /// Black to white.
    case luma
    /// One channel's own axis, through neutral: cyan to red, magenta to green,
    /// yellow to blue. What a wheel's Red slider actually does is take red
    /// *out* on the way down, and taking red out is adding cyan.
    case axis(Rgb8, Rgb8)

    /// The colour at `t` along the track.
    public func at(_ t: Double) -> Rgb8 {
        let t = min(max(Float(t), 0), 1)
        switch self {
        case .plain:
            return Palette.track.rgb
        case .temp:
            return Self.mix3(Rgb8(48, 108, 214), Rgb8(126, 126, 126), Rgb8(232, 196, 88), t)
        case .tint:
            return Self.mix3(Rgb8(62, 190, 104), Rgb8(126, 126, 126), Rgb8(204, 84, 196), t)
        case .hue:
            return Self.hsv(t, 0.80, 0.86)
        // A little over a fifth of the circle either side: wide enough to show
        // which way the neighbours lie, narrow enough that the ends are not
        // some other colour entirely.
        case .hueAround(let deg):
            return Self.hsv((deg / 360.0) + (t - 0.5) * 0.22, 0.80, 0.86)
        case .sat(let vivid):
            return Self.lerp(Rgb8(104, 104, 104), vivid, t)
        // Grey at the left, vivid at the right, at a roughly constant
        // lightness — a saturation control does not change how bright the
        // picture is, and a ramp that got brighter as it got more colourful
        // would say it does.
        case .chroma:
            return Self.hsv(t, t * 0.9, 0.46 + t * 0.4)
        case .luma:
            return Self.lerp(Rgb8(14, 14, 14), Rgb8(236, 236, 236), t)
        case .axis(let neg, let pos):
            return Self.mix3(neg, Rgb8(126, 126, 126), pos, t)
        }
    }

    public var isPlain: Bool { self == .plain }

    // ---- how a ramp is spelled -------------------------------------------

    /// How a ramp is spelled where something outside Swift has to read it.
    ///
    /// `pe_theme::Ramp::tag`, mirrored. Not either language's reflection: the
    /// Rust side's derived `Debug` spelled a saturation ramp
    /// `Sat(Rgb8 { r: 35, g: 228, b: 235 })`, which named a field layout and
    /// would have changed the moment anyone added a field. Both sides write it
    /// out, so changing it is a decision rather than a side effect.
    public var tag: String {
        switch self {
        case .plain: "plain"
        case .temp: "temp"
        case .tint: "tint"
        case .hue: "hue"
        case .hueAround(let deg): "hueAround(\(Self.degrees(deg)))"
        case .sat(let vivid): "sat(\(Self.bytes(vivid)))"
        case .chroma: "chroma"
        case .luma: "luma"
        case .axis(let neg, let pos): "axis(\(Self.bytes(neg)),\(Self.bytes(pos)))"
        }
    }

    /// The ramp the engine spelled `tag`, if this side has one.
    ///
    /// The inverse of ``tag``, and the way the sampled fixture names the ramp
    /// each column of colours belongs to.
    public static func named(_ tag: String) -> Ramp? {
        switch tag {
        case "plain": return .plain
        case "temp": return .temp
        case "tint": return .tint
        case "hue": return .hue
        case "chroma": return .chroma
        case "luma": return .luma
        default: break
        }
        guard let open = tag.firstIndex(of: "("), tag.hasSuffix(")") else { return nil }
        let head = String(tag[tag.startIndex..<open])
        let body = String(tag[tag.index(after: open)..<tag.index(before: tag.endIndex)])
        if head == "hueAround" {
            guard let deg = Float(body) else { return nil }
            return .hueAround(deg)
        }
        let n = body.split(separator: ",").compactMap { UInt8($0) }
        switch (head, n.count) {
        case ("sat", 3):
            return .sat(Rgb8(n[0], n[1], n[2]))
        case ("axis", 6):
            return .axis(Rgb8(n[0], n[1], n[2]), Rgb8(n[3], n[4], n[5]))
        default:
            return nil
        }
    }

    private static func bytes(_ c: Rgb8) -> String { "\(c.r),\(c.g),\(c.b)" }

    /// A hue, in degrees, spelled the way it was written down.
    ///
    /// Every band's hue is a whole number of degrees, so `28` rather than the
    /// `28.0` Swift would print and the `2.8e1` it might. Anything else gets
    /// three places, which is finer than the hue circle can show and is the
    /// same string the engine produces.
    private static func degrees(_ deg: Float) -> String {
        deg.isFinite && deg.truncatingRemainder(dividingBy: 1) == 0 && abs(deg) < 1e9
            ? String(Int(deg))
            : String(format: "%.3f", deg)
    }

    // ---- the arithmetic --------------------------------------------------

    /// HSV to a display colour.
    ///
    /// Hand-rolled on both sides, and hand-rolled the *same* way. The engine
    /// does not go through `egui::ecolor::Hsva`, whose components are *linear*
    /// — `Hsva::new(0.0, 0.85, 0.92)` converts to a display red of 246, not
    /// 235, and every ramp built that way came out looking bleached beside the
    /// hand-picked colours next to it. Reaching for a linear conversion here
    /// instead would put the bleached ramps back on one platform only.
    static func hsv(_ h: Float, _ s: Float, _ v: Float) -> Rgb8 {
        // `f32::rem_euclid(1.0)`: the truncating remainder, lifted back above
        // zero. A hue window centred on red asks for negative hues at its left
        // end, and those have to come round to magenta rather than clamp.
        var h = h.truncatingRemainder(dividingBy: 1.0)
        if h < 0 { h += 1.0 }
        h *= 6.0
        let c = v * s
        let x = c * (1.0 - abs(h.truncatingRemainder(dividingBy: 2.0) - 1.0))
        let rgb: (Float, Float, Float)
        switch h.isFinite ? Int(h) : 0 {
        case 0: rgb = (c, x, 0.0)
        case 1: rgb = (x, c, 0.0)
        case 2: rgb = (0.0, c, x)
        case 3: rgb = (0.0, x, c)
        case 4: rgb = (x, 0.0, c)
        default: rgb = (c, 0.0, x)
        }
        let m = v - c
        return Rgb8(
            Self.byte((rgb.0 + m) * 255.0),
            Self.byte((rgb.1 + m) * 255.0),
            Self.byte((rgb.2 + m) * 255.0)
        )
    }

    static func lerp(_ a: Rgb8, _ b: Rgb8, _ t: Float) -> Rgb8 {
        let f = { (x: UInt8, y: UInt8) -> UInt8 in
            Self.byte(Float(x) + (Float(y) - Float(x)) * t)
        }
        return Rgb8(f(a.r, b.r), f(a.g, b.g), f(a.b, b.b))
    }

    static func mix3(_ a: Rgb8, _ mid: Rgb8, _ b: Rgb8, _ t: Float) -> Rgb8 {
        t < 0.5 ? Self.lerp(a, mid, t * 2.0) : Self.lerp(mid, b, (t - 0.5) * 2.0)
    }

    /// A channel as a byte: rounded half away from zero, then clamped.
    ///
    /// The rounding is where the two implementations could most easily part
    /// company, so it is written once and every ramp goes through it. `Float`
    /// rounds to even at ties in some libraries; `rounded()` and Rust's
    /// `f32::round` both go away from zero, which is what makes the fixture
    /// agree to the byte.
    ///
    /// Not-a-number becomes zero rather than a trap — `f32 as u8` saturates on
    /// the other side, and a ramp handed a bad `t` should draw something wrong
    /// rather than take the application down.
    static func byte(_ f: Float) -> UInt8 {
        guard f.isFinite else { return 0 }
        return UInt8(min(max(f.rounded(), 0), 255))
    }

    // ---- which parameter gets which --------------------------------------

    /// The three channel axes, in the order a wheel's sliders are drawn.
    public static let channelAxes: [Ramp] = [
        .axis(Rgb8(72, 200, 208), Rgb8(226, 78, 72)),
        .axis(Rgb8(210, 76, 190), Rgb8(94, 202, 96)),
        .axis(Rgb8(226, 206, 84), Rgb8(86, 122, 226)),
    ]

    /// The colour a mixer band is named after.
    private static func bandHue(_ name: String) -> Float? {
        switch name {
        case "red": return 0
        case "orange": return 28
        case "yellow": return 52
        case "green": return 110
        case "aqua", "cyan": return 182
        case "blue": return 222
        case "purple": return 272
        case "magenta": return 312
        default: return nil
        }
    }

    /// Which ramp a parameter gets, decided from its key.
    ///
    /// Keyed off the parameter rather than listed per panel because the same
    /// parameter appears in several: Temp. Shift inside Film Damage is the same
    /// axis as Temperature in Basic, and a user who has learnt that
    /// blue-to-yellow means white balance should not have to learn it twice.
    ///
    /// The matches are on whole words, not substrings. `contains("tint")`
    /// looked fine until a `tilt` or a `saturation` inside some unrelated
    /// effect picked up a gradient that made a promise the control does not
    /// keep.
    public static func `for`(effect: String, key: String) -> Ramp {
        // The colour mixer's three rows per band.
        if let underscore = key.firstIndex(of: "_"),
           let deg = bandHue(String(key[key.startIndex..<underscore])) {
            switch String(key[key.index(after: underscore)...]) {
            case "hue": return .hueAround(deg)
            case "saturation", "sat": return .sat(hsv(deg / 360.0, 0.85, 0.92))
            case "luminance", "lum": return .luma
            default: return .plain
            }
        }
        switch (effect, key) {
        case (_, "temperature"), (_, "temp"), (_, "temp_shift"): return .temp
        case (_, "tint"), (_, "tint_shift"): return .tint
        case (_, "hue"), (_, "hue_rotate"): return .hue
        case (_, "saturation"), (_, "vibrance"), (_, "sat"): return .chroma
        default: return .plain
        }
    }
}
