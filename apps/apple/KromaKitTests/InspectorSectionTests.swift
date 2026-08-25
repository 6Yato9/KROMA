import XCTest

/// How the inspector divides one effect's controls up.
///
/// The registry has given every parameter a `section` since it was written, and
/// the panel ignored all of them except the ones the warper claims — which is
/// why Film Damage drew thirty rows in one undifferentiated column with nothing
/// to say where "Grain Params" ended and "Advanced Controls" began.
///
/// Everything here is asked of the registry fixture rather than of a session,
/// so what fails is the grouping and not the engine.
final class InspectorSectionTests: XCTestCase {
    private func registry() throws -> Registry {
        let url = try XCTUnwrap(
            Bundle(for: Self.self).url(forResource: "registry", withExtension: "json"),
            "registry.json is not in the test bundle"
        )
        return try JSONDecoder().decode(Registry.self, from: Data(contentsOf: url))
    }

    private func effect(_ key: String) throws -> Effect {
        try XCTUnwrap(try registry().effect(key), "no effect called \(key)")
    }

    /// Effects in the registry really do declare sections, or none of this
    /// draws anything and every assertion below is vacuous.
    func testTheRegistryHasSectionsToDraw() throws {
        let grouped = try registry().effects.filter { effect in
            InspectorPanel.blocks(of: InspectorPanel.loose(of: effect)).contains {
                if case .section = $0 { return true }
                return false
            }
        }
        XCTAssertGreaterThan(
            grouped.count, 5,
            "only \(grouped.count) effects group anything, so the headers draw nothing")
    }

    /// The grouping itself: every row lands in exactly one block, once.
    func testEveryRowIsDrawnExactlyOnce() throws {
        for effect in try registry().effects {
            let rows = InspectorPanel.loose(of: effect)
            var drawn: [String] = []
            for block in InspectorPanel.blocks(of: rows) {
                switch block {
                case let .loose(param): drawn.append(param.key)
                case let .section(_, params): drawn.append(contentsOf: params.map(\.key))
                }
            }
            XCTAssertEqual(
                drawn, rows.map(\.key),
                "\(effect.key) draws its rows in a different order, or twice, or not at all")
        }
    }

    /// A section takes the place of its first parameter, and everything keeps
    /// the order the registry put it in.
    ///
    /// Grain's Advanced Controls come after its Grain Params in Resolve because
    /// that is the order in the registry. Sorting the sections, or hoisting all
    /// of them below the loose rows, would be this panel deciding an order the
    /// registry already decided.
    func testSectionsKeepTheRegistrysOrder() throws {
        let grain = try effect("grain")
        let rows = InspectorPanel.loose(of: grain)
        let names = InspectorPanel.blocks(of: rows).compactMap { block -> String? in
            if case let .section(name, _) = block { return name }
            return nil
        }
        XCTAssertFalse(names.isEmpty, "grain declares no sections any more")

        // The order the sections first appear in the registry, taken from the
        // registry rather than written down here.
        var expected: [String] = []
        for param in rows where !param.section.isEmpty {
            if !expected.contains(param.section) { expected.append(param.section) }
        }
        XCTAssertEqual(names, expected)
        XCTAssertEqual(Set(names).count, names.count, "a section got two headers")
    }

    /// The warper's sections are drawn by the warper, and must not be drawn
    /// again underneath it.
    ///
    /// `WarperPanel` claims every section holding a lattice or a set of pins
    /// and draws that section's other rows under the grid they govern — which
    /// is where Resolve puts them. A header for the same section below would be
    /// the divisions controls twice.
    func testTheWarperSectionsAreNotDrawnASecondTime() throws {
        let warper = try effect("colour_warper")
        let claimed = InspectorPanel.warpSections(of: warper)
        XCTAssertFalse(claimed.isEmpty, "the warper claims no sections any more")

        for block in InspectorPanel.blocks(of: InspectorPanel.loose(of: warper)) {
            if case let .section(name, _) = block {
                XCTAssertFalse(
                    claimed.contains(name),
                    "\(name) is drawn by the warper's switcher and again as a header")
            }
        }
    }

    /// A parameter with no section stays at the top level rather than being
    /// swept into a heading of its own.
    func testASectionlessParameterGetsNoHeader() throws {
        let exposure = try effect("exposure")
        let blocks = InspectorPanel.blocks(of: InspectorPanel.loose(of: exposure))
        XCTAssertFalse(blocks.isEmpty, "exposure lays out nothing")
        for block in blocks {
            if case let .section(name, _) = block {
                XCTFail("exposure grew a section called \(name)")
            }
        }
    }
}
