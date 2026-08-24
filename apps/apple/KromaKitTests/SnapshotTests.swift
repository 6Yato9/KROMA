import XCTest
// Same module as the code under test; see EngineTests.swift.

final class SnapshotTests: XCTestCase {
    /// The committed fixture, which a Rust test regenerates and checks. Each
    /// test file carries its own copy of this helper so it can be read alone.
    private func fixture(_ name: String) throws -> Data {
        let url = try XCTUnwrap(
            Bundle(for: Self.self).url(forResource: name, withExtension: "json"),
            "\(name).json is not in the test bundle"
        )
        return try Data(contentsOf: url)
    }

    func testTheCommittedSnapshotDecodes() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        XCTAssertTrue(snap.isOpen)
        XCTAssertEqual(snap.width, 64)
        XCTAssertEqual(snap.height, 64)
        XCTAssertEqual(snap.rows.count, 12)
        XCTAssertTrue(snap.canUndo)
        XCTAssertFalse(snap.canRedo)
        XCTAssertEqual(snap.exportFormat, "jpeg")
        XCTAssertEqual(snap.exportQuality, 95)
        XCTAssertEqual(snap.colour.input, "sRGB")
    }

    func testAParameterKeepsTheDocumentsOwnShape() throws {
        // `{"t":"float","v":0.75}` — the same representation the document uses
        // on disk. One shape on the wire, not two.
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        let added = try XCTUnwrap(snap.rows.first { !$0.pinned })
        XCTAssertEqual(added.effect, "exposure")
        XCTAssertEqual(added.params["ev"]?.floatValue, 0.75)
        XCTAssertEqual(added.blend, "normal")
        XCTAssertTrue(added.enabled)
        XCTAssertEqual(added.opacity, 1)
    }

    func testAnEmptySnapshotIsStillASnapshot() throws {
        // The viewer draws its empty state from this rather than from a null.
        let json = Data(#"{"version":0,"is_open":false,"width":0,"height":0,"rows":[],"color":{"input":"","output":""},"passes":0,"can_undo":false,"can_redo":false,"export_format":"jpeg","export_quality":95}"#.utf8)
        let snap = try JSONDecoder().decode(Snapshot.self, from: json)
        XCTAssertFalse(snap.isOpen)
        XCTAssertTrue(snap.rows.isEmpty)
        XCTAssertNil(snap.path)
    }

    func testEveryParameterValueKindDecodes() throws {
        let json = Data(#"""
        {"a":{"t":"float","v":1.5},"b":{"t":"bool","v":true},
         "c":{"t":"choice","v":"Custom"},"d":{"t":"rgb","v":[0.1,0.2,0.3]},
         "e":{"t":"int","v":7}}
        """#.utf8)
        let values = try JSONDecoder().decode([String: ParamValue].self, from: json)
        XCTAssertEqual(values["a"]?.floatValue, 1.5)
        XCTAssertEqual(values["b"], .bool(true))
        XCTAssertEqual(values["c"], .choice("Custom"))
        // An int reads as a float, because every consumer of a number here
        // wants one and the document is the only thing that cares which it was.
        XCTAssertEqual(values["e"]?.floatValue, 7)
        // A curve, a warp and a pin lattice are structure the slice does not
        // draw yet. They must decode as *something* rather than failing the
        // whole snapshot, or one Curves row makes the application unopenable.
        let curve = Data(#"{"k":{"t":"curve","v":{"points":[[0,0],[1,1]]}}}"#.utf8)
        let decoded = try JSONDecoder().decode([String: ParamValue].self, from: curve)
        XCTAssertEqual(decoded["k"], .opaque("curve"))
    }
}
