import Foundation

/// Every effect the engine knows, as data.
///
/// Decoded once at launch from `pe_registry_json()`. The inspector is
/// generated from this rather than written per effect, which is why the
/// Windows shell is thirteen thousand lines and not forty: one `match` on the
/// parameter's kind renders every control for all thirty effects. This is the
/// same trick, in Swift.
public struct Registry: Decodable, Sendable {
    public let effects: [Effect]
    /// The rows a fresh document starts with, shown as fixed panels.
    public let pinned: [String]
    /// Effects that do something visible at their defaults.
    public let visibleAtDefaults: [String]
    /// Group headings, in the order the browser shows them.
    public let groups: [String]

    enum CodingKeys: String, CodingKey {
        case effects, pinned, groups
        case visibleAtDefaults = "visible_at_defaults"
    }

    public func effect(_ key: String) -> Effect? {
        effects.first { $0.key == key }
    }

    /// The pinned effects, in pinned order, skipping any the registry does not
    /// have. A pinned key with no effect behind it is a bug in the engine, not
    /// something for the inspector to crash over.
    public var pinnedEffects: [Effect] {
        pinned.compactMap(effect)
    }
}

public struct Effect: Decodable, Sendable, Identifiable {
    public let key: String
    public let name: String
    public let group: String
    /// `"linear"` or `"log"`. The interface never acts on this; it is here for
    /// the About panel and for bug reports, where "which space was that in" is
    /// the first question anybody asks.
    public let space: String
    public let spatial: Bool
    public let params: [Param]
    public let gates: [Gate]

    public var id: String { key }

    /// Whether a parameter can currently do anything, given the row's values.
    /// A parameter no gate names is always active.
    public func isActive(_ key: String, values: [String: ParamValue]) -> Bool {
        guard let gate = gates.first(where: { $0.params.contains(key) }) else {
            return true
        }
        return gate.isSatisfied(by: values)
    }
}

/// The kind and bounds of one parameter.
///
/// Flat on the wire, an enum here. The JSON keeps `kind` as a string with only
/// the fields that apply — a tagged union is pleasant in Rust and in Swift and
/// unpleasant in the JSON between them.
public enum ParamKind: Sendable {
    case float(Bounds)
    case bool(default: Bool)
    case rgb(default: [Float])
    /// A four-way colour wheel. `master` is whether it has a fourth,
    /// achromatic *readout* — not whether it has an achromatic control, since
    /// every wheel has the ribbed bar under it.
    case wheel(Bounds, master: Bool)
    /// `flat` says what the identity is: a tone curve's is the diagonal, a
    /// secondary's is a level line down the middle. Getting it the wrong way
    /// round makes a freshly added Curves row rotate every hue in the picture.
    case curve(flat: Bool)
    case choice(options: [String], default: String)
    case pins
    case warp
}

/// Where a slider starts, ends, rests, and grows its fill from.
public struct Bounds: Sendable, Equatable {
    public let min: Float
    public let max: Float
    public let `default`: Float
    /// Where "no change" sits — usually but not always the default. The fill
    /// is drawn from here and double-click resets to it.
    public let neutral: Float
}

public struct Param: Decodable, Sendable, Identifiable {
    public let key: String
    public let name: String
    public let kind: ParamKind
    /// Unit suffix for the readout, e.g. `"EV"`, `"K"`, `"%"`.
    public let unit: String
    /// Collapsible heading this belongs under, or `""` for the top level.
    public let section: String

    public var id: String { key }

    enum CodingKeys: String, CodingKey {
        case key, name, kind, unit, section
        case min, max, neutral, options, master, flat
        case defaultFloat = "default_float"
        case defaultBool = "default_bool"
        case defaultRGB = "default_rgb"
        case defaultChoice = "default_choice"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        key = try c.decode(String.self, forKey: .key)
        name = try c.decode(String.self, forKey: .name)
        unit = try c.decodeIfPresent(String.self, forKey: .unit) ?? ""
        section = try c.decodeIfPresent(String.self, forKey: .section) ?? ""

        let tag = try c.decode(String.self, forKey: .kind)
        func bounds() throws -> Bounds {
            Bounds(
                min: try c.decode(Float.self, forKey: .min),
                max: try c.decode(Float.self, forKey: .max),
                default: try c.decode(Float.self, forKey: .defaultFloat),
                neutral: try c.decode(Float.self, forKey: .neutral)
            )
        }

        switch tag {
        case "float":
            kind = .float(try bounds())
        case "bool":
            kind = .bool(default: try c.decode(Bool.self, forKey: .defaultBool))
        case "rgb":
            kind = .rgb(default: try c.decode([Float].self, forKey: .defaultRGB))
        case "wheel":
            kind = .wheel(try bounds(), master: try c.decode(Bool.self, forKey: .master))
        case "curve":
            kind = .curve(flat: try c.decode(Bool.self, forKey: .flat))
        case "choice":
            kind = .choice(
                options: try c.decode([String].self, forKey: .options),
                default: try c.decode(String.self, forKey: .defaultChoice)
            )
        case "pins":
            kind = .pins
        case "warp":
            kind = .warp
        default:
            // A kind this build has never heard of. Refused loudly rather than
            // skipped: a control that silently does not appear is a parameter
            // the user cannot reach and cannot be told about.
            throw DecodingError.dataCorruptedError(
                forKey: .kind,
                in: c,
                debugDescription: "unknown parameter kind \(tag)"
            )
        }
    }
}

/// What has to be true for a group of parameters to apply.
public struct Gate: Decodable, Sendable {
    public let by: String
    public let when: When
    /// Set only when `when` is `.is`.
    public let option: String?
    public let params: [String]

    public enum When: String, Decodable, Sendable {
        case isTrue = "true"
        case isFalse = "false"
        case positive
        case `is`
        case drawn
    }

    public init(by: String, when: When, option: String?, params: [String]) {
        self.by = by
        self.when = when
        self.option = option
        self.params = params
    }

    /// Whether the controls this gate guards can currently do anything.
    ///
    /// A gate naming a parameter that is not there, or one of the wrong kind,
    /// must not silently disable what it guards — that would be a typo taking
    /// a third of a panel away with no error. So the default is active.
    public func isSatisfied(by values: [String: ParamValue]) -> Bool {
        guard let current = values[by] else { return true }
        switch (when, current) {
        case (.isTrue, .bool(let v)): return v
        case (.isFalse, .bool(let v)): return !v
        case (.positive, .float(let v)): return abs(v) > 1e-6
        case (.is, .choice(let v)): return v == option
        case (.drawn, _): return true
        default: return true
        }
    }
}
