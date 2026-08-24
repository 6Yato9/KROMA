import XCTest
// No import: these sources compile into the test bundle, so the tests are
// already in the same module. `@testable import KromaKit` is accepted but
// ignored, with a warning on every compile.

final class RegistryTests: XCTestCase {
    /// The committed fixture, which a Rust test regenerates and checks.
    /// Decoding *this* rather than calling the engine is the point: if a field
    /// is added in Rust and not here, this fails rather than the application
    /// quietly losing a control.
    private func fixture(_ name: String) throws -> Data {
        let url = try XCTUnwrap(
            Bundle(for: Self.self).url(forResource: name, withExtension: "json"),
            "\(name).json is not in the test bundle"
        )
        return try Data(contentsOf: url)
    }

    func testTheWholeRegistryDecodes() throws {
        let registry = try JSONDecoder().decode(Registry.self, from: fixture("registry"))
        XCTAssertEqual(registry.effects.count, 30)
        XCTAssertEqual(registry.pinned.count, 11)
        XCTAssertEqual(registry.groups, ["Basic", "Colour", "Film", "Optics"])
    }

    func testEveryParameterKindSurvivesTheCrossing() throws {
        // Eight kinds, eight control views. A kind that fails to decode is a
        // control that silently never appears.
        let registry = try JSONDecoder().decode(Registry.self, from: fixture("registry"))
        var seen = Set<String>()
        for effect in registry.effects {
            for param in effect.params {
                switch param.kind {
                case .float: seen.insert("float")
                case .bool: seen.insert("bool")
                case .rgb: seen.insert("rgb")
                case .wheel: seen.insert("wheel")
                case .curve: seen.insert("curve")
                case .choice: seen.insert("choice")
                case .pins: seen.insert("pins")
                case .warp: seen.insert("warp")
                }
            }
        }
        XCTAssertEqual(
            seen,
            ["float", "bool", "rgb", "wheel", "curve", "choice", "pins", "warp"]
        )
    }

    func testAFloatCarriesEverythingASliderNeeds() throws {
        let registry = try JSONDecoder().decode(Registry.self, from: fixture("registry"))
        let exposure = try XCTUnwrap(registry.effect("exposure"))
        let ev = try XCTUnwrap(exposure.params.first { $0.key == "ev" })
        guard case let .float(bounds) = ev.kind else {
            return XCTFail("ev is not a float")
        }
        // Without all four a slider cannot draw itself: where it starts, where
        // it ends, where it rests, and where its fill grows from.
        XCTAssertEqual(bounds.min, -5)
        XCTAssertEqual(bounds.max, 5)
        XCTAssertEqual(bounds.default, 0)
        XCTAssertEqual(bounds.neutral, 0)
        XCTAssertEqual(ev.unit, "EV")
        XCTAssertEqual(ev.name, "Exposure")
    }

    func testAChoiceKnowsItsOptions() throws {
        let registry = try JSONDecoder().decode(Registry.self, from: fixture("registry"))
        let param = try XCTUnwrap(
            registry.effects.flatMap(\.params).first {
                if case .choice = $0.kind { return true } else { return false }
            }
        )
        guard case let .choice(options, fallback) = param.kind else {
            return XCTFail("not a choice")
        }
        XCTAssertFalse(options.isEmpty)
        XCTAssertTrue(options.contains(fallback))
    }

    func testAGateNamesWhatItSwitchesOff() throws {
        let registry = try JSONDecoder().decode(Registry.self, from: fixture("registry"))
        let gated = try XCTUnwrap(registry.effects.first { !$0.gates.isEmpty })
        let gate = try XCTUnwrap(gated.gates.first)
        XCTAssertFalse(gate.by.isEmpty)
        XCTAssertFalse(gate.params.isEmpty)
    }

    func testAControlGatedOnACheckboxIsInactiveUntilItIsTicked() throws {
        // Resolve greys out controls that cannot do anything, and so does the
        // Windows shell. A panel of forty controls where a third silently do
        // nothing teaches the user wrong things about the effect.
        let gate = Gate(by: "enabled", when: .isTrue, option: nil, params: ["amount"])
        XCTAssertFalse(gate.isSatisfied(by: ["enabled": .bool(false)]))
        XCTAssertTrue(gate.isSatisfied(by: ["enabled": .bool(true)]))
    }
}
