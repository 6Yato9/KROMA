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

    /// The size an export will be, which the File page shows beside the
    /// source's.
    ///
    /// Decoding at all is most of the assertion: both fields are
    /// non-optional, so a key spelt differently on the two sides throws here
    /// rather than quietly reading zero. The fixture's session has no crop and
    /// no turn, so the two sizes agree in it — that they *can* differ is
    /// asserted on the Rust side, where a document can be cropped without a
    /// GPU.
    func testTheCommittedSnapshotCarriesTheOutputSize() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        XCTAssertEqual(snap.outputWidth, 64)
        XCTAssertEqual(snap.outputHeight, 64)
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
        //
        // The geometry block is here because the engine always writes one: it
        // is not optional on the wire, and with nothing open it is the
        // identity rather than absent. A decoder that let it go missing would
        // read a cropped photograph as an uncropped one on the day the two
        // sides disagreed about the key. The output size is here for the same
        // reason: the engine writes it whether or not anything is open, and
        // with nothing open it is the source's nothing rather than absent.
        let json = Data(#"{"version":0,"is_open":false,"width":0,"height":0,"output_width":0,"output_height":0,"rows":[],"color":{"input":"","output":""},"geometry":{"centre":[0.0,0.0],"size":[1.0,1.0],"angle":0.0,"turns":0,"flip_h":false,"flip_v":false,"aspect":"free"},"passes":0,"can_undo":false,"can_redo":false,"export_format":"jpeg","export_quality":95}"#.utf8)
        let snap = try JSONDecoder().decode(Snapshot.self, from: json)
        XCTAssertFalse(snap.isOpen)
        XCTAssertTrue(snap.rows.isEmpty)
        XCTAssertNil(snap.path)
        XCTAssertTrue(snap.geometry.isIdentity)
    }

    func testAGeometryDecodesItsSevenFields() throws {
        let json = Data(#"""
        {"centre":[0.1,-0.05],"size":[0.3,0.4],"angle":12.5,"turns":3,
         "flip_h":true,"flip_v":false,"aspect":"ratio","aspect_w":16.0,"aspect_h":9.0}
        """#.utf8)
        let g = try JSONDecoder().decode(GeometryValue.self, from: json)
        XCTAssertEqual(g.centre.x, 0.1, accuracy: 0.0001)
        XCTAssertEqual(g.centre.y, -0.05, accuracy: 0.0001)
        XCTAssertEqual(g.size.width, 0.3, accuracy: 0.0001)
        XCTAssertEqual(g.size.height, 0.4, accuracy: 0.0001)
        XCTAssertEqual(g.angle, 12.5, accuracy: 0.0001)
        XCTAssertEqual(g.turns, 3)
        XCTAssertTrue(g.flipH)
        XCTAssertFalse(g.flipV)
        XCTAssertEqual(g.aspect, .ratio(w: 16, h: 9))
        XCTAssertFalse(g.isIdentity)
    }

    func testEveryAspectLockDecodes() throws {
        // Three arms, spread across three keys: the string names the arm and
        // the two numbers are there only for a ratio.
        func lock(_ body: String) throws -> AspectLock {
            let head = #"{"centre":[0,0],"size":[1,1],"angle":0,"turns":0,"flip_h":false,"flip_v":false,"#
            let json = Data((head + body + "}").utf8)
            return try JSONDecoder().decode(GeometryValue.self, from: json).aspect
        }
        XCTAssertEqual(try lock(#""aspect":"free""#), .free)
        XCTAssertEqual(try lock(#""aspect":"original""#), .original)
        XCTAssertEqual(
            try lock(#""aspect":"ratio","aspect_w":3.0,"aspect_h":2.0"#), .ratio(w: 3, h: 2))

        // A ratio with nothing to hold is not a ratio. The engine never writes
        // one, and reading it as a lock would divide by a number that is not
        // there.
        XCTAssertEqual(try lock(#""aspect":"ratio""#), .free)

        // And an arm this build has never heard of reads as free rather than
        // refusing the whole snapshot — the same reason `ParamValue` has
        // `.opaque`. A photograph written by a later version still opens.
        XCTAssertEqual(try lock(#""aspect":"golden""#), .free)
    }

    func testALockedRatioAnswersWithItsProportion() throws {
        // The number the crop arithmetic wants, from the two the panel prints.
        XCTAssertEqual(try XCTUnwrap(AspectLock.ratio(w: 16, h: 9).widthOverHeight),
                       16.0 / 9.0, accuracy: 0.0001)
        // Free has no fixed ratio, and Original's is the source's — which only
        // the engine knows, so this side does not guess at it.
        XCTAssertNil(AspectLock.free.widthOverHeight)
        XCTAssertNil(AspectLock.original.widthOverHeight)
    }

    func testAGeometryIsIdentityOnlyWhenItLeavesThePictureAlone() {
        XCTAssertTrue(GeometryValue.identity.isIdentity)

        // A lock is not a change to the picture — it constrains the next drag.
        // `Geometry::is_identity` leaves it out for that reason, and so does
        // this one.
        XCTAssertTrue(
            GeometryValue(
                centre: .zero, size: CGSize(width: 1, height: 1), angle: 0, turns: 0,
                flipH: false, flipV: false, aspect: .ratio(w: 1, h: 1)
            ).isIdentity)

        // Four quarter-turns is none, which is what the engine's modulo says.
        XCTAssertTrue(
            GeometryValue(
                centre: .zero, size: CGSize(width: 1, height: 1), angle: 0, turns: 4,
                flipH: false, flipV: false, aspect: .free
            ).isIdentity)

        for changed in [
            GeometryValue(centre: CGPoint(x: 0.1, y: 0), size: CGSize(width: 1, height: 1),
                          angle: 0, turns: 0, flipH: false, flipV: false, aspect: .free),
            GeometryValue(centre: .zero, size: CGSize(width: 0.9, height: 1),
                          angle: 0, turns: 0, flipH: false, flipV: false, aspect: .free),
            GeometryValue(centre: .zero, size: CGSize(width: 1, height: 1),
                          angle: 0.5, turns: 0, flipH: false, flipV: false, aspect: .free),
            GeometryValue(centre: .zero, size: CGSize(width: 1, height: 1),
                          angle: 0, turns: 1, flipH: false, flipV: false, aspect: .free),
            GeometryValue(centre: .zero, size: CGSize(width: 1, height: 1),
                          angle: 0, turns: 0, flipH: true, flipV: false, aspect: .free),
            GeometryValue(centre: .zero, size: CGSize(width: 1, height: 1),
                          angle: 0, turns: 0, flipH: false, flipV: true, aspect: .free),
        ] {
            XCTAssertFalse(changed.isIdentity, "\(changed) reads as doing nothing")
        }
    }

    func testTheCommittedSnapshotCarriesAnIdentityGeometry() throws {
        // Nothing in the fixture crops, so the block is the whole frame. If it
        // ever decodes as something else, the two sides have parted company
        // about what an uncropped photograph looks like.
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        XCTAssertTrue(snap.geometry.isIdentity)
        XCTAssertEqual(snap.geometry.aspect, .free)
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
        // Every kind the engine writes today reads as itself — a warp as a
        // lattice, a pin set as pins. The `.opaque` fallback stays for a kind
        // this build has never heard of, because a document written by a later
        // version must still open: the Colour Warper is pinned, so a decoder
        // that refused an unknown kind would make that photograph unopenable.
        let warp = Data(#"""
        {"k":{"t":"warp","v":{"cols":2,"rows":1,"offsets":[[0.0,0.0],[0.0,0.0]]}},
         "p":{"t":"pins","v":[]},
         "z":{"t":"nebula","v":{"anything":true}}}
        """#.utf8)
        let decoded = try JSONDecoder().decode([String: ParamValue].self, from: warp)
        XCTAssertEqual(decoded["k"], .warp(WarpValue(cols: 2, rows: 1, offsets: [.zero, .zero])))
        XCTAssertEqual(decoded["p"], .pins([]))
        XCTAssertEqual(decoded["z"], .opaque("nebula"))
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
        // And pins reads as a pin set now, not as opaque structure.
        guard case .pins = try XCTUnwrap(warper.params["pins"]) else {
            return XCTFail("pins is not a pin set")
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

    func testPinsDecodeTheirFields() throws {
        let json = Data(#"""
        {"k":{"t":"pins","v":[{"at":[0.33,0.35],"to":[0.40,0.30],"chroma_range":0.12,
        "tonal_low":0.2,"tonal_high":0.9,"tonal_pivot":0.6,"exposure":0.75}]}}
        """#.utf8)
        let values = try JSONDecoder().decode([String: ParamValue].self, from: json)
        guard case let .pins(pins) = try XCTUnwrap(values["k"]) else {
            return XCTFail("not a pin set")
        }
        XCTAssertEqual(pins.count, 1)
        let p = try XCTUnwrap(pins.first)
        XCTAssertEqual(p.at.x, 0.33, accuracy: 0.0001)
        XCTAssertEqual(p.to.y, 0.30, accuracy: 0.0001)
        XCTAssertEqual(p.chromaRange, 0.12, accuracy: 0.0001)
        XCTAssertEqual(p.tonalLow, 0.2, accuracy: 0.0001)
        XCTAssertEqual(p.tonalHigh, 0.9, accuracy: 0.0001)
        XCTAssertEqual(p.tonalPivot, 0.6, accuracy: 0.0001)
        XCTAssertEqual(p.exposure, 0.75, accuracy: 0.0001)
    }

    func testAnEmptyPinSetDecodes() throws {
        // Every fresh document has one, and it is the shape the committed
        // snapshot carries.
        let json = Data(#"{"k":{"t":"pins","v":[]}}"#.utf8)
        let values = try JSONDecoder().decode([String: ParamValue].self, from: json)
        guard case let .pins(pins) = try XCTUnwrap(values["k"]) else {
            return XCTFail("not a pin set")
        }
        XCTAssertTrue(pins.isEmpty)
    }

    func testAPlacedPinIsNeutralAndADraggedOneIsNot() {
        // A pin placed and not yet moved is not a no-op waiting to happen — it
        // is one the user has put somewhere deliberately and is about to move.
        // It reads as neutral so the row costs nothing until it does something.
        let placed = PinValue(at: CGPoint(x: 0.33, y: 0.35), to: CGPoint(x: 0.33, y: 0.35),
                              chromaRange: 0.04, tonalLow: 1, tonalHigh: 1,
                              tonalPivot: 0.5, exposure: 0)
        XCTAssertTrue(placed.isNeutral)

        let dragged = PinValue(at: placed.at, to: CGPoint(x: 0.4, y: 0.3),
                               chromaRange: placed.chromaRange, tonalLow: placed.tonalLow,
                               tonalHigh: placed.tonalHigh, tonalPivot: placed.tonalPivot,
                               exposure: placed.exposure)
        XCTAssertFalse(dragged.isNeutral)

        // Exposure counts too. A pin can be dead centre and still be
        // brightening the picture, and a neutrality check that only looked at
        // the drag would let the renderer skip a row that is doing something.
        let lifted = PinValue(at: placed.at, to: placed.at, chromaRange: placed.chromaRange,
                              tonalLow: placed.tonalLow, tonalHigh: placed.tonalHigh,
                              tonalPivot: placed.tonalPivot, exposure: 0.5)
        XCTAssertFalse(lifted.isNeutral)
    }

    func testTheCommittedSnapshotCarriesAReadableEmptyPinSet() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        let warper = try XCTUnwrap(snap.rows.first { $0.effect == "colour_warper" })
        guard case let .pins(pins) = try XCTUnwrap(warper.params["pins"]) else {
            return XCTFail("pins is not a pin set")
        }
        XCTAssertTrue(pins.isEmpty, "a fresh warper has no pins")
    }

    func testThePinLimitMatchesTheEngine() throws {
        // Two numbers that have to be one: the Add button stops at Swift's,
        // the session refuses past the engine's. The fixture carries the
        // engine's, so a change on that side fails here rather than offering a
        // ninth pin nothing will accept.
        let raw = try JSONSerialization.jsonObject(with: fixture("pin_samples")) as? [String: Any]
        XCTAssertEqual(try XCTUnwrap(raw)["max_pins"] as? Int, PinValue.maxPins)
    }
}
