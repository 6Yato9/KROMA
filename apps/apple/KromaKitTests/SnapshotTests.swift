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
        // A warp reads as a lattice now. A pin lattice is still structure the
        // slice does not draw, and must decode as *something* rather than
        // failing the whole snapshot: the Colour Warper is pinned, so a
        // decoder that refused pins would make every photograph unopenable.
        let warp = Data(#"""
        {"k":{"t":"warp","v":{"cols":2,"rows":1,"offsets":[[0.0,0.0],[0.0,0.0]]}},
         "p":{"t":"pins","v":[]}}
        """#.utf8)
        let decoded = try JSONDecoder().decode([String: ParamValue].self, from: warp)
        XCTAssertEqual(decoded["k"], .warp(WarpValue(cols: 2, rows: 1, offsets: [.zero, .zero])))
        XCTAssertEqual(decoded["p"], .opaque("pins"))
    }

    func testAWheelDecodesItsFourComponents() throws {
        // Resolve's wheels are four-valued: three channels and the luminance
        // ring around the outside. The master is modelled separately rather
        // than folded into the channels, so that resetting just the ring stays
        // possible — the same reason pe-core keeps them apart.
        let json = Data(#"{"k":{"t":"wheel","v":{"master":1.0,"rgb":[0.25,0.5,0.75]}}}"#.utf8)
        let values = try JSONDecoder().decode([String: ParamValue].self, from: json)
        guard case let .wheel(w) = try XCTUnwrap(values["k"]) else {
            return XCTFail("not a wheel")
        }
        XCTAssertEqual(w.master, 1.0, accuracy: 0.0001)
        XCTAssertEqual(w.rgb[0], 0.25, accuracy: 0.0001)
        XCTAssertEqual(w.rgb[1], 0.5, accuracy: 0.0001)
        XCTAssertEqual(w.rgb[2], 0.75, accuracy: 0.0001)
    }

    func testACurveDecodesItsPoints() throws {
        let json = Data(#"{"k":{"t":"curve","v":[[0.0,0.0],[0.5,0.7],[1.0,1.0]]}}"#.utf8)
        let values = try JSONDecoder().decode([String: ParamValue].self, from: json)
        guard case let .curve(c) = try XCTUnwrap(values["k"]) else {
            return XCTFail("not a curve")
        }
        XCTAssertEqual(c.points.count, 3)
        XCTAssertEqual(c.points[1].x, 0.5, accuracy: 0.0001)
        XCTAssertEqual(c.points[1].y, 0.7, accuracy: 0.0001)
    }

    func testTheCommittedSnapshotCarriesReadableCurves() throws {
        // Custom Curves is pinned, so every fresh document has ten of them. If
        // they decode as opaque the panel cannot draw.
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        let curves = try XCTUnwrap(snap.rows.first { $0.effect == "curves" })
        guard case let .curve(luma) = try XCTUnwrap(curves.params["luma"]) else {
            return XCTFail("luma is not a curve")
        }
        // A tone curve's identity is the diagonal.
        XCTAssertEqual(try XCTUnwrap(luma.points.first).y, 0, accuracy: 0.0001)
        XCTAssertEqual(try XCTUnwrap(luma.points.last).y, 1, accuracy: 0.0001)

        guard case let .curve(hue) = try XCTUnwrap(curves.params["hue_vs_hue"]) else {
            return XCTFail("hue_vs_hue is not a curve")
        }
        // A secondary's identity is a level line down the middle — a different
        // question, and a different answer.
        XCTAssertEqual(try XCTUnwrap(hue.points.first).y, 0.5, accuracy: 0.0001)
        XCTAssertEqual(try XCTUnwrap(hue.points.last).y, 0.5, accuracy: 0.0001)
    }

    func testAWarpDecodesItsGridAndOffsets() throws {
        let json = Data(#"{"k":{"t":"warp","v":{"cols":2,"rows":3,"offsets":[[0,0],[0.1,0.2],[0,0],[0,0],[0,0],[-0.3,0.4]]}}}"#.utf8)
        let values = try JSONDecoder().decode([String: ParamValue].self, from: json)
        guard case let .warp(w) = try XCTUnwrap(values["k"]) else {
            return XCTFail("not a warp")
        }
        XCTAssertEqual(w.cols, 2)
        XCTAssertEqual(w.rows, 3)
        XCTAssertEqual(w.offsets.count, 6)
        // Row-major, so index 1 is column 1 of row 0.
        XCTAssertEqual(w.at(col: 1, row: 0).x, 0.1, accuracy: 0.0001)
        XCTAssertEqual(w.at(col: 1, row: 2).y, 0.4, accuracy: 0.0001)
    }

    func testAVertexOutsideTheGridReadsAsNoDisplacement() {
        // Matching `Warp::at`, which returns [0, 0] rather than trapping. A
        // view asking for a vertex that is not there is a bug, but blanking
        // the panel over it would be a worse one.
        let w = WarpValue(cols: 2, rows: 2, offsets: [
            CGPoint(x: 0.5, y: 0.5), .zero, .zero, .zero,
        ])
        XCTAssertEqual(w.at(col: 9, row: 0), .zero)
        XCTAssertEqual(w.at(col: 0, row: 9), .zero)
        XCTAssertEqual(w.at(col: -1, row: 0), .zero)
    }

    func testTheCommittedSnapshotCarriesReadableLattices() throws {
        // The Colour Warper is pinned, so every fresh document has three.
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        let warper = try XCTUnwrap(snap.rows.first { $0.effect == "colour_warper" })
        for key in ["hue_sat", "chroma_luma_1", "chroma_luma_2"] {
            guard case let .warp(w) = try XCTUnwrap(warper.params[key]) else {
                return XCTFail("\(key) is not a warp")
            }
            XCTAssertEqual(w.cols, 6)
            XCTAssertEqual(w.rows, 6)
            XCTAssertEqual(w.offsets.count, 36)
            XCTAssertTrue(w.isIdentity, "a fresh lattice should leave the picture alone")
        }
        // And pins still decodes as opaque — it is the next plan, not this one.
        guard case .opaque = try XCTUnwrap(warper.params["pins"]) else {
            return XCTFail("pins should still be opaque")
        }
    }

    func testTheCommittedSnapshotCarriesReadableWheels() throws {
        // primaries and log_wheels are pinned, so every fresh document has
        // wheels in it. If they decode as opaque the panels cannot draw.
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        let primaries = try XCTUnwrap(snap.rows.first { $0.effect == "primaries" })
        guard case let .wheel(gain) = try XCTUnwrap(primaries.params["gain"]) else {
            return XCTFail("gain is not a wheel")
        }
        XCTAssertEqual(gain.master, 1.0, accuracy: 0.0001)
    }

    func testReplacingAVertexLeavesTheRestAlone() {
        let w = WarpValue(cols: 2, rows: 2, offsets: [.zero, .zero, .zero, .zero])
        let moved = w.replacing(col: 1, row: 1, with: CGPoint(x: 0.3, y: 0.4))
        XCTAssertEqual(moved.at(col: 1, row: 1).x, 0.3, accuracy: 0.0001)
        XCTAssertEqual(moved.at(col: 0, row: 0), .zero)
        XCTAssertEqual(moved.cols, 2)
        // And a vertex the grid does not have changes nothing rather than
        // growing the array.
        XCTAssertEqual(w.replacing(col: 9, row: 0, with: CGPoint(x: 1, y: 1)).offsets.count, 4)
    }
}
