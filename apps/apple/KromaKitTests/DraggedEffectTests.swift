import UniformTypeIdentifiers
import XCTest
// Same module as the code under test; see EngineTests.swift.

final class DraggedEffectTests: XCTestCase {
    /// Our own type, not `public.text`.
    ///
    /// A `String` payload would mean a word dragged out of any other
    /// application lands on the picture and adds an effect — or, more often,
    /// looks as though it will and then does nothing, which is worse.
    func testTheTypeIsOursAndNotText() {
        XCTAssertEqual(DraggedEffect.type.identifier, "com.kroma.effect-key")
        XCTAssertFalse(
            DraggedEffect.type.conforms(to: .plainText),
            "a plain-text drag would satisfy this drop target")
        XCTAssertFalse(DraggedEffect.type.conforms(to: .fileURL))
    }

    // Whether the system has *registered* the identifier is not asserted here,
    // and deliberately: registration comes from the application bundle's
    // `UTExportedTypeDeclarations`, and these tests run in a unit-test bundle
    // that is not that application. A test for it would fail for a reason that
    // has nothing to do with the code. The declaration lives in `project.yml`
    // and is checked by the drag working.

    /// It survives the round trip the drag performs.
    func testItRoundTrips() throws {
        let sent = DraggedEffect(key: "halation")
        let got = try DraggedEffect.decoded(from: sent.encoded())
        XCTAssertEqual(got, sent)
        XCTAssertEqual(got.key, "halation")
    }

    /// And something that is not one of ours does not decode into one.
    func testForeignDataDoesNotDecode() {
        XCTAssertThrowsError(try DraggedEffect.decoded(from: Data("halation".utf8)))
        XCTAssertThrowsError(try DraggedEffect.decoded(from: Data("{}".utf8)))
    }
}
