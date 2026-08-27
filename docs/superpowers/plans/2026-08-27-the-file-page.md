# The File Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the Mac the fourth and last Windows tab — the File page: what the photograph is, what it will be written as, and the two settings that decide it.

**Architecture:** The Windows shell has exactly four tabs — `Colour`, `Effects`, `Image`, `File` (`apps/windows/src/main.rs:1754`). The Mac's icon strip already covers the first three: `Colour` is the five pinned tools, `Effects` is `Tool::Effects`, `Image` is `Tool::Crop`. `File` has no counterpart at all. It becomes the eighth tool in the strip.

The export API is already wired end to end and simply has no control on it: `pe_session_set_export` → `Engine.setExport(format:quality:)` → `SessionStore.setExport(format:quality:)`, and the snapshot already carries `export_format`/`export_quality` back (`apps/apple/KromaKit/Snapshot.swift:45`). Nothing in the Mac UI calls the setter, so a Mac export always uses the default. This plan adds the panel that drives it.

Two things move into shared code on the way, following the pattern of the eight ports before it. `Format::ALL` and `Format::takes_quality` replace a hardcoded three-element array and a `== Format::Jpeg` comparison in the Windows shell, and a committed fixture stops the Mac's copy of the list from drifting. The output size — which differs from the source size the moment a crop exists — joins the snapshot rather than being recomputed in Swift.

**Tech Stack:** Rust (`pe-session`, `pe-effects`), egui (Windows shell), Swift/SwiftUI (`KromaKit`), XCTest, committed JSON fixtures.

---

## File Structure

**Rust — shared:**
- `crates/pe-session/src/export.rs` — gains `Format::ALL` and `Format::takes_quality`. The list of formats and the rule about which one has a quality becomes one answer instead of two.
- `crates/pe-session/src/describe.rs` — `Snapshot` gains `output_width`/`output_height`.
- `crates/pe-effects/src/tool.rs` — gains `Tool::File`, last.

**Rust — Windows shell:**
- `apps/windows/src/main.rs` — `export_section` reads `Format::ALL` and `takes_quality()` instead of spelling both out.

**Rust — fixtures:**
- `crates/pe-session/tests/fixtures.rs` — a new `export_formats.json`; `theme.json` and `snapshot.json` regenerate.

**Swift:**
- `apps/apple/KromaKit/Snapshot.swift` — decodes the two new fields.
- `apps/apple/KromaKit/Controls/ToolStrip.swift` — mirrors `Tool::File`.
- `apps/apple/KromaKit/Controls/FilePanel.swift` — **new.** The panel: five info rows, the format menu, the quality slider.
- `apps/apple/PhotoEditor/ContentView.swift` — one `case .file` in the switch that already has `.effects` and `.crop`.
- `apps/apple/KromaKitTests/FilePanelTests.swift` — **new.**
- `apps/apple/KromaKitTests/ToolStripTests.swift`, `SnapshotTests.swift` — updated.

`FilePanel.swift` is its own file for the reason `CropPanel.swift` is: a tool with no rows behind it draws itself, and does not belong in the registry-generated inspector.

---

### Task 1: The formats, listed once

**Files:**
- Modify: `crates/pe-session/src/export.rs:32-68`
- Modify: `apps/windows/src/main.rs:2127` (the `for format in [...]` line) and the `is_jpeg` line below it
- Test: `crates/pe-session/src/export.rs` (the existing `#[cfg(test)] mod tests` at the bottom of the file)

**Context:** The Windows shell writes `for format in [Format::Jpeg, Format::Png, Format::Png16]`. A fourth format added to the enum compiles fine and silently never appears in the picker. This is exactly what `Tool::ALL` and `Group::ALL` exist to prevent, and the Mac is about to need the same list — so it moves next to the enum.

`takes_quality` moves for the same reason: Windows spells the rule as `chosen.format == Format::Jpeg`, and the Mac would spell it a second time. It is one answer per format.

- [ ] **Step 1: Write the failing test**

Add to the test module at the bottom of `crates/pe-session/src/export.rs`:

```rust
    /// `ALL` is a hand-written list next to an enum, which is the shape that
    /// goes stale. The match is exhaustive, so a variant added and not listed
    /// here fails to compile rather than quietly vanishing from both pickers.
    #[test]
    fn all_lists_every_format() {
        for format in Format::ALL {
            match format {
                Format::Jpeg | Format::Png | Format::Png16 => {}
            }
        }
        assert_eq!(Format::ALL.len(), 3);
        // And no duplicates, which a copy-paste into the array would give.
        for (i, a) in Format::ALL.iter().enumerate() {
            for b in &Format::ALL[i + 1..] {
                assert_ne!(a, b, "{a:?} is in ALL twice");
            }
        }
    }

    /// The quality is a JPEG idea. Stated here rather than as a comparison in
    /// each shell, because two shells disagreeing about which formats have a
    /// quality is a slider that does nothing on one of them.
    #[test]
    fn only_jpeg_takes_a_quality() {
        assert!(Format::Jpeg.takes_quality());
        assert!(!Format::Png.takes_quality());
        assert!(!Format::Png16.takes_quality());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p pe-session --lib export
```

Expected: FAIL — `no associated item named ALL found for enum Format`, `no method named takes_quality`.

- [ ] **Step 3: Write the implementation**

In `crates/pe-session/src/export.rs`, inside `impl Format`, above `extension`:

```rust
    /// Every format, in the order a picker should offer them.
    ///
    /// Here rather than in a shell because there are two shells. A variant
    /// added and not listed is a format nobody can choose, and the only
    /// symptom is its absence.
    pub const ALL: [Format; 3] = [Format::Jpeg, Format::Png, Format::Png16];

    /// Whether the quality setting means anything for this format.
    ///
    /// The shells grey the control rather than hiding it: a row that vanishes
    /// takes its explanation with it, and the row staying put, dimmed, says
    /// "quality is a JPEG idea" far better than an empty space does.
    pub fn takes_quality(self) -> bool {
        self == Format::Jpeg
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p pe-session --lib export
```

Expected: PASS.

- [ ] **Step 5: Point the Windows shell at them**

In `apps/windows/src/main.rs`, in `export_section`, replace the hardcoded array:

```rust
        for format in Format::ALL {
```

and replace the `is_jpeg` line — read the current line first, it reads `let is_jpeg = chosen.format == Format::Jpeg;` — with:

```rust
    let takes_quality = chosen.format.takes_quality();
```

Then rename its uses in the lines below (there is at least one, guarding the dimmed row). Keep the surrounding comment about greying rather than hiding — it is still true and now describes a rule the engine states.

- [ ] **Step 6: Build the Windows shell to prove it still compiles**

The engine has been broken on Windows before by a change nobody cross-compiled. Check the target is installed, then check the shell:

```bash
cargo check -p photo-editor --target x86_64-pc-windows-msvc 2>&1 | tail -5 || cargo check -p photo-editor 2>&1 | tail -5
```

Expected: `Finished`. If the Windows target is not installed, the fallback `cargo check -p photo-editor` still type-checks the shell's source, which is what changed.

- [ ] **Step 7: Commit**

```bash
git add crates/pe-session/src/export.rs apps/windows/src/main.rs && git commit -m "The formats are listed once, and the quality rule with them"
```

---

### Task 2: The formats cross to Swift

**Files:**
- Modify: `crates/pe-session/tests/fixtures.rs` (add a test beside `the_theme_fixture_is_current`)
- Create: `apps/apple/Fixtures/export_formats.json` (generated, then committed)

**Context:** The fixtures are the seam that stops the two halves of the application diverging in silence — `registry.json`, `theme.json`, `locus.json` and six others already work this way. The Mac's format picker needs the same three formats with the same labels and the same FFI names, and a fixture is what makes a disagreement a test failure instead of a bug report.

- [ ] **Step 1: Write the fixture test**

Add to `crates/pe-session/tests/fixtures.rs`:

```rust
/// The formats a picker offers, with the strings each side needs.
///
/// `name` is what crosses the FFI, `label` is what the reader sees, and
/// `takes_quality` is whether the quality row is live. Three strings that must
/// agree across two shells, so they are generated from the engine rather than
/// typed twice.
#[test]
fn the_export_formats_fixture_is_current() {
    use pe_session::export::Format;

    let formats: Vec<serde_json::Value> = Format::ALL
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.name(),
                "label": f.label(),
                "extension": f.extension(),
                "takes_quality": f.takes_quality(),
            })
        })
        .collect();

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "formats": formats,
        // The default a session starts at, so the Swift test can assert the
        // panel's opening state rather than assuming it.
        "default_format": Format::default().name(),
    }))
    .unwrap();
    check("export_formats.json", json);
}
```

If `pe_session::export::Format` is not reachable at that path, check how `Format` is re-exported at the top of `crates/pe-session/src/lib.rs` and use whatever path the crate actually publishes. Do not add a new `pub use` for the test's convenience.

- [ ] **Step 2: Run it to verify it fails**

```bash
cargo test -p pe-session --test fixtures the_export_formats_fixture_is_current
```

Expected: FAIL — `apps/apple/Fixtures/export_formats.json is missing — run with PE_UPDATE_FIXTURES=1`.

- [ ] **Step 3: Generate the fixture**

```bash
PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures the_export_formats_fixture_is_current
```

- [ ] **Step 4: Read the generated file and check it by eye**

```bash
cat "apps/apple/Fixtures/export_formats.json"
```

Expected: three entries — `jpeg`/`JPEG`/`jpg`/`true`, `png`/`PNG 8`/`png`/`false`, `png16`/`PNG 16`/`png`/`false` — and `"default_format": "jpeg"`. If any of that is wrong, the bug is in Task 1, not in the fixture.

- [ ] **Step 5: Run it again to verify it now passes**

```bash
cargo test -p pe-session --test fixtures
```

Expected: PASS, all fixture tests.

- [ ] **Step 6: Commit**

```bash
git add crates/pe-session/tests/fixtures.rs "apps/apple/Fixtures/export_formats.json" && git commit -m "The formats cross as a fixture"
```

---

### Task 3: The output size joins the snapshot

**Files:**
- Modify: `crates/pe-session/src/describe.rs:225-243` (the `Snapshot` struct) and the builder around line 344-367
- Modify: `apps/apple/KromaKit/Snapshot.swift` (the properties, `CodingKeys`, `empty`, and `init`)
- Test: `crates/pe-session/tests/` — put it wherever the existing snapshot/describe tests live; find them with `grep -rn "fn.*snapshot" crates/pe-session/tests/ crates/pe-session/src/describe.rs`
- Test: `apps/apple/KromaKitTests/SnapshotTests.swift`

**Context:** The File page shows both sizes because they differ: the source is the file's pixels, the output is what `render_full` will actually produce once the crop and the resize have had their say. `pe_render::export::output_size(doc, w, h)` already answers it, and `pe-session` already depends on `pe-render` (`crates/pe-session/Cargo.toml:14`). Computing it a second time in Swift would be a copy of a rule that lives in `pe_core::Geometry` and `pe_core::Resize`.

- [ ] **Step 1: Write the failing Rust test**

The point of the test is that the new fields are not just the source size copied. A crop is what makes them differ, so the test crops. Add it beside the other describe tests:

```rust
    /// The output is not the source once anything has been cropped, which is
    /// the entire reason the File page shows both numbers.
    #[test]
    fn the_snapshot_reports_the_cropped_output_size() {
        let mut s = pe_session::Session::new();
        s.open_test_chart(256, 256).unwrap();

        let before = pe_session::describe::snapshot(&s);
        assert_eq!((before.output_width, before.output_height), (256, 256));

        // The left half of the picture.
        let mut geometry = before.geometry.clone();
        geometry.crop = pe_core::Rect {
            x: 0.0,
            y: 0.0,
            w: 0.5,
            h: 1.0,
        };
        s.set_geometry(geometry).unwrap();

        let after = pe_session::describe::snapshot(&s);
        assert_eq!((after.width, after.height), (256, 256), "the source is unchanged");
        assert_eq!((after.output_width, after.output_height), (128, 256));
    }
```

The exact spelling of the geometry type is what `Snapshot::geometry` carries — `GeometryJson` — and of `set_geometry`'s argument. Read `crates/pe-session/src/session.rs` for `set_geometry`'s signature and `describe.rs` for `GeometryJson`'s fields before writing this, and adjust the middle of the test to match. The two assertions are the point and must not change: 256×256 source, 128×256 output.

- [ ] **Step 2: Run it to verify it fails**

```bash
cargo test -p pe-session the_snapshot_reports_the_cropped_output_size
```

Expected: FAIL — no field `output_width` on `Snapshot`.

- [ ] **Step 3: Add the fields**

In `crates/pe-session/src/describe.rs`, in `pub struct Snapshot`, after `pub height: u32`:

```rust
    /// What an export will actually produce.
    ///
    /// Not the source's size: the crop decides how much picture there is and
    /// the resize decides how many pixels it is delivered in. Carried rather
    /// than recomputed in each shell, because it is
    /// [`pe_render::export::output_size`] and that is where the rule lives.
    pub output_width: u32,
    pub output_height: u32,
```

In the builder, beside where `width`/`height` are filled:

```rust
    let (output_width, output_height) =
        pe_render::export::output_size(session.document(), width, height);
```

placed after `width` and `height` are known, and then `output_width,` / `output_height,` in the struct literal. Read the surrounding code for what the source dimensions are called there and what accessor gives the `Document` — the `file_page` in `apps/windows/src/main.rs:1971` does the same call and is the reference.

- [ ] **Step 4: Run it to verify it passes**

```bash
cargo test -p pe-session the_snapshot_reports_the_cropped_output_size
```

Expected: PASS.

- [ ] **Step 5: Prove the test discriminates**

A test that would pass against the wrong implementation is not a test. Temporarily change the builder to `let (output_width, output_height) = (width, height);`, run it, confirm FAIL on `(128, 256)`, then put it back.

```bash
cargo test -p pe-session the_snapshot_reports_the_cropped_output_size
```

Expected while broken: FAIL, `assertion (256, 256) == (128, 256)`.

- [ ] **Step 6: Regenerate the snapshot fixture**

```bash
PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures the_snapshot_fixture_is_current && git diff --stat "apps/apple/Fixtures/snapshot.json"
```

Expected: two lines added, `output_width` and `output_height`. Anything else means something unintended changed — stop and look.

- [ ] **Step 7: Teach Swift the two fields**

In `apps/apple/KromaKit/Snapshot.swift`, after `public let height: UInt32`:

```swift
    /// What an export will produce — the crop and the resize applied to the
    /// source. `width`/`height` is the file; this is the result.
    public let outputWidth: UInt32
    public let outputHeight: UInt32
```

Add to `CodingKeys`:

```swift
        case outputWidth = "output_width"
        case outputHeight = "output_height"
```

Add `outputWidth: 0, outputHeight: 0` to `empty`, and the two parameters to `init` in the same position they hold in the struct, assigning both.

- [ ] **Step 8: Add the Swift test**

In `apps/apple/KromaKitTests/SnapshotTests.swift`, following whatever pattern the existing tests there use to load `snapshot.json`:

```swift
    /// Decoded from the fixture rather than asserted as a constant: the point
    /// is that the engine's field names and Swift's `CodingKeys` agree.
    func testDecodesTheOutputSize() throws {
        let snapshot = try decoded()
        XCTAssertEqual(snapshot.outputWidth, snapshot.width)
        XCTAssertEqual(snapshot.outputHeight, snapshot.height)
    }
```

`decoded()` is a stand-in — use whichever helper the file already has. The fixture's session has no crop, so the two sizes match there; the *decoding* is what this asserts, and a missing key would throw.

- [ ] **Step 9: Run both suites**

```bash
cargo test -p pe-session 2>&1 | tail -5
```

```bash
"apps/apple/run-tests.sh" 2>&1 | tail -20
```

Use whatever the repo's Swift test script is actually called — check `ls apps/apple/*.sh` — and expect 0 failures from both.

- [ ] **Step 10: Commit**

```bash
git add -A && git commit -m "The snapshot carries the size an export will be"
```

---

### Task 4: File joins the strip

**Files:**
- Modify: `crates/pe-effects/src/tool.rs` — the module doc, the enum, `ALL`, `name`, `effects`, and the `only_effects_and_crop_own_nothing_pinned` test
- Modify: `apps/apple/KromaKit/Controls/ToolStrip.swift` — the doc, the enum, `effects`, `symbol`
- Modify: `apps/apple/KromaKitTests/ToolStripTests.swift`
- Regenerate: `apps/apple/Fixtures/theme.json`

**Context:** `Tool` is the icon strip's contents, and the strip is the Mac's tab bar — it already carries `Effects` and `Crop`, which are Windows' Effects and Image tabs. `File` is the fourth. It sits last, after Crop, in the order Windows lists its tabs.

Two doc comments become wrong when this lands and must be fixed, not left: the module doc says "One page of the colour tools", and `Crop`'s doc says "It sits last". Both were true of seven tools.

- [ ] **Step 1: Update the failing test first**

In `crates/pe-effects/src/tool.rs`, find `only_effects_and_crop_own_nothing_pinned` and rewrite it to name the third:

```rust
    /// Three tools own no pinned effects, and each for its own reason:
    /// Effects draws whatever the user added, Crop edits the document's
    /// geometry, and File is about the file rather than the picture.
    #[test]
    fn only_effects_crop_and_file_own_nothing_pinned() {
        for tool in Tool::ALL {
            let empty = tool.effects().is_empty();
            let should_be_empty =
                matches!(tool, Tool::Effects | Tool::Crop | Tool::File);
            assert_eq!(empty, should_be_empty, "{tool:?}");
        }
    }
```

Read the existing test first and keep its shape where it is better than this — the requirement is only that `File` is accounted for and that a tool wrongly owning effects still fails.

- [ ] **Step 2: Run it to verify it fails**

```bash
cargo test -p pe-effects tool
```

Expected: FAIL to compile — `no variant named File`.

- [ ] **Step 3: Add the variant**

In `crates/pe-effects/src/tool.rs`:

```rust
    /// What the photograph is, and what it will be written as.
    ///
    /// The one tool that is about the *file* rather than the picture, so it
    /// owns no pinned effects for the same reason Crop does not: there is no
    /// `Effect` behind it. It sits last, after Crop, in the order the Windows
    /// shell lists its tabs — Colour, Effects, Image, File.
    File,
```

`ALL` becomes `[Tool; 8]` with `Tool::File` appended. `name()` gains `Tool::File => "File"`. `effects()` gains `Tool::File => &[]` — put it with `Effects` and `Crop`, and widen their shared comment to say three rather than two.

Fix the module doc's "One page of the colour tools" — the strip is no longer only colour tools — and delete "It sits last" from `Crop`'s doc, since File does now.

- [ ] **Step 4: Run it to verify it passes**

```bash
cargo test -p pe-effects
```

Expected: PASS. If a test elsewhere asserts seven tools, that test is now wrong and should be updated to eight — but read it first; if it asserts something about *colour* tools specifically, the right fix may be to exclude File rather than to bump a number.

- [ ] **Step 5: Regenerate the theme fixture**

```bash
PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures the_theme_fixture_is_current && git diff "apps/apple/Fixtures/theme.json"
```

Expected: one entry added — `{ "name": "File", "effects": [] }`.

- [ ] **Step 6: Mirror it in Swift**

In `apps/apple/KromaKit/Controls/ToolStrip.swift`, after `case crop`:

```swift
    /// What the photograph is, and what it will be written as.
    ///
    /// Owns no pinned effects, like Effects and Crop: `FilePanel` is what it
    /// draws. Last, in the order the Windows shell lists its tabs.
    case file = "File"
```

Add `case .file: []` to `effects`, beside `.effects` and `.crop`, and widen their comment to name all three.

Add to `symbol`:

```swift
        case .file: "doc"
```

`doc` is SF Symbols 1.0. `ToolStripTests` asks `NSImage` for every symbol, so a name the system lacks fails there rather than rendering a blank button — do not skip Step 8.

Update the type's own doc: "the same seven tools" becomes eight, and "The colour page's tools" no longer describes a strip with File in it.

- [ ] **Step 7: Update the Swift test's counts**

In `apps/apple/KromaKitTests/ToolStripTests.swift`, the comment at line 48 says "seven buttons and six" — make it eight and seven. Any literal `7` becomes `8`. The fixture comparison at lines 59-62 counts from the fixture and needs no change.

- [ ] **Step 8: Run the Swift suite**

```bash
"apps/apple/run-tests.sh" 2>&1 | tail -20
```

Expected: 0 failures. The symbol test proves `doc` renders; the fixture test proves the eight tools match the engine's eight.

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "File is the eighth tool"
```

---

### Task 5: The panel

**Files:**
- Create: `apps/apple/KromaKit/Controls/FilePanel.swift`
- Create: `apps/apple/KromaKitTests/FilePanelTests.swift`

**Context:** `CropPanel.swift` is the model — a tool with no rows behind it, drawing its own labelled rows against `RowMetrics`, using `ChoiceMenu` from `Chrome.swift` for a choice and `ScalarRow` from `ParameterRow.swift` for a number. `ScalarRow` is already used by the pin controls with no registry parameter behind it, so it takes the quality directly.

Read `apps/apple/KromaKit/Controls/CropPanel.swift` in full before writing this. Match its `label(_:)` helper, its row heights, and its spacing rather than inventing new ones.

The information comes from `store.snapshot`: `name`, `path`, `width`/`height`, `outputWidth`/`outputHeight`, `exportFormat`, `exportQuality`. The set position comes from the library — check `apps/apple/KromaKit/Library.swift` and `SessionStore` for what is already exposed (`store.library` and a current index); if the count and position are not reachable, show the other four rows and leave the fifth out rather than adding new plumbing in this task.

- [ ] **Step 1: Write the failing test**

`FilePanel` is a view, so test the decisions rather than the pixels — the same thing `CropPanelTests` does. Create `apps/apple/KromaKitTests/FilePanelTests.swift`:

```swift
import XCTest
@testable import KromaKit

final class FilePanelTests: XCTestCase {
    /// The three formats, their labels and their FFI names, against the
    /// engine's own list. Two shells offering different formats is the drift
    /// this fixture exists to catch.
    func testTheFormatsMatchTheEngine() throws {
        let url = try XCTUnwrap(
            Bundle(for: Self.self).url(forResource: "export_formats", withExtension: "json"))
        let json = try JSONSerialization.jsonObject(with: Data(contentsOf: url))
        let root = try XCTUnwrap(json as? [String: Any])
        let formats = try XCTUnwrap(root["formats"] as? [[String: Any]])

        XCTAssertEqual(formats.count, ExportFormat.all.count)
        for (i, entry) in formats.enumerated() {
            let format = ExportFormat.all[i]
            XCTAssertEqual(entry["name"] as? String, format.name)
            XCTAssertEqual(entry["label"] as? String, format.label)
            XCTAssertEqual(entry["takes_quality"] as? Bool, format.takesQuality)
        }
    }

    /// A format the engine sent that Swift does not know is still shown, and
    /// still named — the alternative is a menu whose current value is absent
    /// from its own options, which draws as blank.
    func testAnUnknownFormatStillNamesItself() {
        XCTAssertEqual(ExportFormat.label(of: "png16"), "PNG 16")
        XCTAssertEqual(ExportFormat.label(of: "webp"), "webp")
    }

    /// The quality row is live for a JPEG and dead for the two PNGs. Read from
    /// the same property the panel dims itself with, so the test cannot pass
    /// while the panel is wrong.
    func testOnlyJpegTakesAQuality() {
        XCTAssertTrue(ExportFormat.takesQuality(name: "jpeg"))
        XCTAssertFalse(ExportFormat.takesQuality(name: "png"))
        XCTAssertFalse(ExportFormat.takesQuality(name: "png16"))
    }
}
```

The fixture must be in the test bundle's resources. Check how `ToolStripTests` reaches `theme.json` — there is an existing mechanism (`fixture(_ name:)` at `ToolStripTests.swift:27`) and `export_formats.json` needs adding to it, which may mean a line in the Xcode project or the package manifest. Follow whatever the other eight fixtures do.

- [ ] **Step 2: Run it to verify it fails**

```bash
"apps/apple/run-tests.sh" 2>&1 | tail -20
```

Expected: FAIL — `cannot find 'ExportFormat' in scope`.

- [ ] **Step 3: Write `ExportFormat`**

At the top of `apps/apple/KromaKit/Controls/FilePanel.swift`:

```swift
import SwiftUI

/// The formats an export may be written as.
///
/// A mirror of `pe_session::export::Format`, checked against it by
/// `export_formats.json`: the same three, in the same order, with the same
/// labels and the same rule about which has a quality.
///
/// The engine's name is the identity rather than a Swift case, because the
/// name is what crosses the FFI in both directions — the snapshot returns one
/// and `setExport` takes one. A name this build does not know is a format a
/// newer engine has; it is shown as itself rather than dropped.
public struct ExportFormat: Sendable, Equatable {
    public let name: String
    public let label: String
    public let takesQuality: Bool

    public static let all: [ExportFormat] = [
        ExportFormat(name: "jpeg", label: "JPEG", takesQuality: true),
        ExportFormat(name: "png", label: "PNG 8", takesQuality: false),
        ExportFormat(name: "png16", label: "PNG 16", takesQuality: false),
    ]

    /// The label for an engine name, or the name itself if this build has
    /// never heard of it.
    ///
    /// Never `nil`: a `ChoiceMenu` whose chosen value is not among its options
    /// draws an empty button, so an unknown format has to name itself.
    public static func label(of name: String) -> String {
        all.first { $0.name == name }?.label ?? name
    }

    public static func name(ofLabel label: String) -> String {
        all.first { $0.label == label }?.name ?? label
    }

    /// Whether the quality control is live. An unknown format is assumed not
    /// to take one — the quality is JPEG's, and a guess that greys a live
    /// control is better than one that offers a setting with no effect.
    public static func takesQuality(name: String) -> Bool {
        all.first { $0.name == name }?.takesQuality ?? false
    }
}
```

- [ ] **Step 4: Run it to verify it passes**

```bash
"apps/apple/run-tests.sh" 2>&1 | tail -20
```

Expected: PASS for the three new tests.

- [ ] **Step 5: Write the panel**

Below `ExportFormat` in the same file. Read `CropPanel.swift` first and match it; this is the shape, not the letter:

```swift
/// The File page: what the photograph is, and what it will be written as.
///
/// Windows' fourth tab. Settings rather than a dialog, and deliberately: a
/// dialog asks the same question every time and is answered the same way every
/// time, where a panel states the answer, keeps it, and stays out of the way of
/// somebody exporting sixty frames.
public struct FilePanel: View {
    @ObservedObject var store: SessionStore

    public init(store: SessionStore) {
        self.store = store
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            SectionTitle("File")
            ForEach(information, id: \.0) { row in
                infoRow(row.0, row.1)
            }
            Hairline()
            SectionTitle("Export")
            formatRow
            qualityRow
        }
    }
}
```

`SectionTitle` and `Hairline` are stand-ins — use whatever `CropPanel` uses for a heading and a rule. The five information rows are `Name`, `Folder`, `Source`, `Output`, `In the set`, in that order, matching `file_page`. A folder is long and must truncate at the head (`.truncationMode(.head)`) so the last component stays readable; a name must not be allowed to push the panel wider than it is.

The format row is a `ChoiceMenu` over `ExportFormat.all.map(\.label)`, chosen `ExportFormat.label(of: store.snapshot.exportFormat)`, calling `store.setExport(format:quality:)` with `ExportFormat.name(ofLabel:)` and the **current** quality — `setExport` takes both, so passing a stale quality silently resets it.

The quality row is a `ScalarRow` with `name: "Quality"`, `unit: ""`, `value: Float(store.snapshot.exportQuality)`, `bounds: Bounds(min: 1, max: 100, default: 95, neutral: 95)`, `ramp: .plain`, and `isActive: ExportFormat.takesQuality(name: store.snapshot.exportFormat)`. Its `onChange` calls `setExport` with the current format and the rounded, clamped quality. `onBegin`/`onEnd` have no undo pairing to do here — export settings are not document history — so they are empty closures unless `ScalarRow` needs otherwise.

Confirm `ScalarRow`'s `isActive` actually dims and disables rather than only dimming; if it only dims, guard the `onChange` as well so a drag on a PNG's quality changes nothing.

- [ ] **Step 6: Add a test for the stale-quality trap**

This is the mistake the row invites, so it gets a test:

```swift
    /// Choosing a format must carry the quality across. `setExport` takes both,
    /// so sending a format with a default quality silently discards whatever
    /// the user had set.
    func testChoosingAFormatKeepsTheQuality() {
        let store = SessionStore.forTesting()
        store.setExport(format: "jpeg", quality: 60)
        XCTAssertEqual(store.snapshot.exportQuality, 60)

        store.setExport(format: "png", quality: store.snapshot.exportQuality)
        XCTAssertEqual(store.snapshot.exportFormat, "png")
        XCTAssertEqual(store.snapshot.exportQuality, 60, "the quality was reset")
    }
```

`SessionStore.forTesting()` is a stand-in — use however `SessionStoreTests` builds a store. If a store cannot be built in a test without a GPU, drop this test and instead assert the panel's helper that computes the arguments, extracting it as a static function so it is reachable.

- [ ] **Step 7: Run the suite**

```bash
"apps/apple/run-tests.sh" 2>&1 | tail -20
```

Expected: 0 failures.

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "The File panel"
```

---

### Task 6: The panel on screen

**Files:**
- Modify: `apps/apple/PhotoEditor/ContentView.swift:155-172` (the `switch tool`)

**Context:** One case, beside `.effects` and `.crop`. `Tool.file` draws no rows, so falling through to `default` would draw an empty column.

- [ ] **Step 1: Add the case**

In `ContentView.swift`, in the `switch tool`, after `case .crop:`:

```swift
                    case .file:
                        // Like Crop, no rows behind it: this is about the file
                        // and the settings it will be written with, neither of
                        // which is an entry in the document's stack.
                        FilePanel(store: store)
```

- [ ] **Step 2: Build the application**

```bash
"apps/apple/build.sh" 2>&1 | tail -20
```

Use whatever the repo's build script is — check `ls apps/apple/*.sh`. Expected: a successful build, no warnings introduced.

- [ ] **Step 3: Run everything**

```bash
cargo test --workspace 2>&1 | tail -8
```

```bash
"apps/apple/run-tests.sh" 2>&1 | tail -20
```

Expected: 0 failures from both. The Rust count should be 802 plus the four tests this plan adds, and Swift 335 plus its three or four.

- [ ] **Step 4: Look at it**

The last several features have not been checked by eye. Copy the build somewhere local — the repo is on a NAS mount and nobody will be running it from one — and open it:

```bash
caffeinate -u -t 2 && open "$HOME/Desktop/Kroma.app"
```

Adjust to wherever the build script actually puts the bundle. Click the eighth icon and confirm: five information rows with real values, a format menu that reads JPEG, and a quality slider that is live. Switch to PNG 8 and confirm the quality row greys. Switch back and confirm the quality is still whatever it was, not 95.

Then open a photograph, crop it with the Crop tool, and return to File: `Output` must differ from `Source`.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "The File page is on screen"
```

---

## Notes for whoever executes this

- **Build from a local path, not the NAS mount.** Copy the bundle off `/Volumes/Projects` before running it.
- **The mount flaps.** A `getcwd` EPERM or an unreadable `.git` is the SMB share, not your change. Retry.
- **Regenerating a fixture is a decision.** `PE_UPDATE_FIXTURES=1` rewrites the file; read `git diff` before committing it. A fixture diff you did not expect is the seam doing its job.
