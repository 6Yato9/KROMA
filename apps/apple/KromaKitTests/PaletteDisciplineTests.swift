import XCTest

/// Whether the views actually *use* the palette.
///
/// `PaletteTests` holds the palette itself to the engine's, name by name and
/// byte by byte, which is the half of the problem the language can check. What
/// it cannot see is a view that never asks the palette anything — a
/// `.quaternary` track, a `.tint` fill, a `Color(white: 0.28)` well. Every one
/// of those compiles, looks approximately right on the machine it was written
/// on, and follows whatever the reader has set in System Settings. That is how
/// four greys became three different SwiftUI defaults on one screen with a
/// green accent through the middle of them, and nothing in the test suite could
/// fail on it.
///
/// So this reads the sources. Grepping Swift with a fixed list of spellings is
/// crude — it knows nothing about scope, and a colour computed at run time gets
/// past it — but it is exactly the check that was missing, and every spelling
/// below is one that was really in this tree before the palette landed.
final class PaletteDisciplineTests: XCTestCase {

    /// The spellings that are not ours, and what each should have been.
    ///
    /// Every one resolves from the *system*: the appearance the reader chose,
    /// the accent colour they picked, the material AppKit happens to draw a bar
    /// with today. A control painted from one of them shares a palette with
    /// nothing else in the application, and cannot be made to.
    ///
    /// Absolute colours — `.white`, `.black`, `Color(hue:…)` — are deliberately
    /// *not* here. A translucent white graticule over a scope and a hue circle
    /// under a lattice are pictures of something, drawn the same way on the
    /// Windows side; they are not the interface reaching for a colour it has no
    /// name for.
    private static let banned: [(needle: String, instead: String)] = [
        (".quaternary", "RULE, TRACK or GRID, depending on what the line is"),
        (".tertiary", "DIM"),
        (".secondary", "LABEL"),
        (".primary", "TITLE, or HANDLE on a control"),
        ("accentColor", "ACCENT for what is doing something, SELECT for what is chosen"),
        ("Color.gray", "one of the four greys, or TRACK"),
        ("Color(white:", "a named grey: VIEWER, WELL, PANEL, RAISED"),
        ("foregroundStyle(.tint)", "ACCENT"),
        ("fill(.tint)", "ACCENT"),
        ("background(.bar)", "PANEL"),
        ("Material", "PANEL, or WELL inside a plot"),
        ("windowBackgroundColor", "PANEL"),
        ("controlBackgroundColor", "CONTROL"),
        ("controlColor", "CONTROL"),
        ("separatorColor", "RULE"),
        ("labelColor", "LABEL"),
        (".toggleStyle(.checkbox)", "KromaCheckboxStyle"),
        (".toggleStyle(.button)", "KromaToggleButtonStyle"),
        (".buttonStyle(.bordered)", "KromaButtonStyle"),
        ("pickerStyle(.segmented)", "ChoiceChips"),
        ("pickerStyle(.menu)", "ChoiceMenu"),
    ]

    /// What the line reaches for that the palette does not have, if anything.
    ///
    /// Comment lines are skipped, because half the point of the palette is
    /// that the files explain what they are *not* doing and why — this one
    /// included.
    static func offence(in line: String) -> String? {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard !trimmed.hasPrefix("//") else { return nil }
        for (needle, instead) in banned where line.contains(needle) {
            return "\(needle) — use \(instead)"
        }
        return nil
    }

    // ---- what the check is pointed at -------------------------------------

    /// The two source directories, found from this file rather than from a
    /// working directory the test runner does not have.
    private static var sourceRoots: [URL] {
        let apple = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()  // KromaKitTests
            .deletingLastPathComponent()  // apps/apple
        return [
            apple.appendingPathComponent("KromaKit"),
            apple.appendingPathComponent("PhotoEditor"),
        ]
    }

    /// Every Swift file that draws, and none that only names colours.
    private static func sources() throws -> [URL] {
        var found: [URL] = []
        for root in sourceRoots {
            let walker = FileManager.default.enumerator(
                at: root, includingPropertiesForKeys: nil)
            guard let walker else { continue }
            for case let url as URL in walker where url.pathExtension == "swift" {
                // `Palette.swift` is where the colours are written down, and
                // the doc comments in it name several of the spellings below.
                if url.lastPathComponent == "Palette.swift" { continue }
                found.append(url)
            }
        }
        return found.sorted { $0.path < $1.path }
    }

    // ---- the check --------------------------------------------------------

    func testNoViewReachesOutsideThePalette() throws {
        var offences: [String] = []
        let files = try Self.sources()
        for file in files {
            let text = try String(contentsOf: file, encoding: .utf8)
            for (i, line) in text.components(separatedBy: .newlines).enumerated() {
                if let why = Self.offence(in: line) {
                    offences.append("\(file.lastPathComponent):\(i + 1): \(why)")
                }
            }
        }
        XCTAssertEqual(
            offences, [],
            "these draw in a colour the palette does not have:\n"
                + offences.joined(separator: "\n"))
    }

    // ---- and the checks on the check --------------------------------------
    //
    // A grep test that reads no files passes, and a grep test whose needles no
    // longer match anything passes. Both of those are the failure mode of this
    // whole idea, so both are asserted rather than assumed.

    /// The scan is pointed at the sources and not at an empty directory.
    func testTheScanReadsTheSourcesItIsFor() throws {
        let files = try Self.sources()
        XCTAssertGreaterThan(
            files.count, 20,
            "the scan found \(files.count) Swift files, which is not this application")

        // Named files rather than a count alone: a path that resolved to the
        // test directory would also be "more than twenty files".
        let names = Set(files.map(\.lastPathComponent))
        for wanted in [
            "ContentView.swift", "InspectorPanel.swift", "ParameterRow.swift",
            "StackRowView.swift", "WheelView.swift", "ScopeViews.swift",
            "CurveEditor.swift", "WarpEditor.swift", "Chrome.swift",
        ] {
            XCTAssertTrue(names.contains(wanted), "the scan never looked at \(wanted)")
        }
    }

    /// The needles still catch what they are for.
    ///
    /// Every line here was in this tree before this palette landed — the wheel's
    /// track, the pins editor's chosen pin, the warper's grey, the status bar's
    /// material. If a needle stops matching its own line, the check has quietly
    /// become an assertion that nothing is wrong.
    func testTheScanCatchesTheThingsThatWereReallyThere() {
        for line in [
            "                .foregroundStyle(isActive ? .primary : .tertiary)",
            "                Capsule().fill(.quaternary).frame(height: 3)",
            "                    .foregroundStyle(row.enabled ? .secondary : .tertiary)",
            "        let tint: Color = on ? .accentColor : .white.opacity(0.82)",
            "                .fill(moved ? Color.accentColor : Color.white.opacity(0.85))",
            "                        colors: [Color(white: 0.5), Color(white: 0.5).opacity(0)],",
            "        .background(.bar)",
            "            .toggleStyle(.checkbox)",
            "            .pickerStyle(.segmented)",
        ] {
            XCTAssertNotNil(
                Self.offence(in: line), "the scan would not have caught: \(line)")
        }
    }

    /// And do not catch what the palette actually looks like in use, nor the
    /// comments that explain what a file is not doing.
    func testTheScanLeavesTheRealThingAlone() {
        for line in [
            "                .fill(Palette.track.color)",
            "        .foregroundStyle(Palette.label.color)",
            "            .background(Palette.panel.color)",
            "        case .tint:",
            "            .toggleStyle(KromaCheckboxStyle())",
            "                .stroke(.white.opacity(0.24), lineWidth: 1)",
            "/// `.toggleStyle(.checkbox)` is an `NSButton`, which means the system accent",
            "        // and `.toggleStyle(.button)` painted it in the system accent — the",
        ] {
            XCTAssertNil(Self.offence(in: line), "the scan flagged: \(line)")
        }
    }
}
