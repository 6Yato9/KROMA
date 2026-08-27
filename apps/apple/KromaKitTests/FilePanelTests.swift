import XCTest
// Same module as the code under test; see EngineTests.swift.

final class FilePanelTests: XCTestCase {
    /// The committed fixture, which a Rust test regenerates and checks. Each
    /// test file carries its own copy of this helper so it can be read alone.
    private func fixture(_ name: String) throws -> [String: Any] {
        let url = try XCTUnwrap(
            Bundle(for: Self.self).url(forResource: name, withExtension: "json"),
            "\(name).json is not in the test bundle"
        )
        let json = try JSONSerialization.jsonObject(with: Data(contentsOf: url))
        return try XCTUnwrap(json as? [String: Any])
    }

    /// The formats, their labels and their FFI names, against the engine's own
    /// list. Two shells offering different formats — or the same format under
    /// two names — is exactly the drift this fixture exists to catch.
    func testTheFormatsMatchTheEngine() throws {
        let formats = try XCTUnwrap(fixture("export_formats")["formats"] as? [[String: Any]])
        XCTAssertEqual(formats.count, ExportFormat.all.count)
        for (i, entry) in formats.enumerated() {
            let format = ExportFormat.all[i]
            XCTAssertEqual(entry["name"] as? String, format.name)
            XCTAssertEqual(entry["label"] as? String, format.label)
            XCTAssertEqual(entry["takes_quality"] as? Bool, format.takesQuality)
        }
    }

    /// And the one a session opens on, so the panel's opening state is the
    /// engine's rather than a guess.
    func testTheDefaultFormatIsKnown() throws {
        let name = try XCTUnwrap(fixture("export_formats")["default_format"] as? String)
        XCTAssertNotNil(ExportFormat.all.first { $0.name == name })
    }

    /// A format the engine sent that this build does not know still names
    /// itself. A `ChoiceMenu` whose chosen value is absent from its options
    /// draws an empty button, which is worse than an unfamiliar word.
    func testAnUnknownFormatStillNamesItself() {
        XCTAssertEqual(ExportFormat.label(of: "png16"), "PNG 16")
        XCTAssertEqual(ExportFormat.label(of: "webp"), "webp")
        XCTAssertEqual(ExportFormat.name(ofLabel: "PNG 16"), "png16")
        XCTAssertEqual(ExportFormat.name(ofLabel: "WebP"), "WebP")
    }

    /// The quality row is live for a JPEG and dead for both PNGs — read
    /// through the same call the panel dims itself with, so the test cannot
    /// pass while the panel is wrong.
    func testOnlyJpegTakesAQuality() {
        XCTAssertTrue(ExportFormat.takesQuality(name: "jpeg"))
        XCTAssertFalse(ExportFormat.takesQuality(name: "png"))
        XCTAssertFalse(ExportFormat.takesQuality(name: "png16"))
        // An unknown format is assumed not to take one.
        XCTAssertFalse(ExportFormat.takesQuality(name: "webp"))
    }
}
