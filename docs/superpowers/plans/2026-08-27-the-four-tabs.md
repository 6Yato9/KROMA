# The Four Tabs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Mac inspector the Windows inspector — four tabs, a header naming the photograph, the effect browser as a shelf rather than a popover, and an effect draggable from that shelf onto the stack or onto the picture.

**Architecture:** The Mac grew an eight-icon strip where Windows has four tabs (`apps/windows/src/main.rs:1754`). Windows' Colour tab is five *collapsing sections* inside one tab; the Mac promoted all five to peers of Effects and Crop, then added File as an eighth. This plan puts them back one level down.

The model splits in two, which is what the Windows shell has always had and the Mac never named:

- **`Tab`** — Colour, Effects, Image, File. What the tab row chooses between.
- **`Section`** — Curves, Basic, Colour Warper, Colour Wheels, Colour Mixer. The five collapsing headers *inside* the Colour tab, in Windows' order and with Windows' default-open states.

`pe_effects::Tool` becomes `Section` and loses the three variants that were never sections — Effects, Crop and File are tabs. The `theme.json` fixture carries both lists, so the Swift mirrors of each cannot drift.

With the browser a shelf inside the Effects tab rather than a popover, dragging becomes possible: a tile can be dragged onto the stack list above it or onto the picture beside it, which is the gesture `apps/windows/src/inspector.rs:128` and `apps/windows/src/main.rs:1608` implement.

**Tech Stack:** Rust (`pe-effects`, `pe-session`), egui (reference implementation), Swift/SwiftUI (`KromaKit`), XCTest, committed JSON fixtures.

---

## What this plan does NOT do

Stated so the next reader does not think they were missed. All three are Windows toolbar features, not inspector layout, and each is its own piece of work:

- **The Grade menu** — Copy, Paste, Paste to all (`main.rs:1230`). It is Windows-shell state (`self.clipboard: Option<Stack>`) and porting it needs `Session::copy_grade`/`paste_grade`/`paste_grade_to_all` across the ABI first.
- **Fit / 100% / the zoom readout** (`main.rs:1310`). `SessionStore.fitView()` exists; the buttons and the percentage do not.
- **The GPU name** (`main.rs:1339`), which the Mac never asks the engine for.

---

## File Structure

**Rust:**
- `crates/pe-effects/src/tool.rs` → `crates/pe-effects/src/tab.rs` — `Tab` (4) and `Section` (5), replacing `Tool` (8).
- `crates/pe-effects/src/lib.rs` — the module rename and re-exports.
- `crates/pe-session/tests/fixtures.rs` — `theme.json` gains `tabs`, and `tools` becomes `sections`.

**Swift:**
- `apps/apple/KromaKit/Controls/ToolStrip.swift` → `apps/apple/KromaKit/Controls/TabRow.swift` — `Tab`, `Section`, and the row of four.
- `apps/apple/KromaKit/InspectorHeader.swift` — **new.** The photograph's name and size, above the tabs.
- `apps/apple/KromaKit/EffectBrowser.swift` — the popover becomes an inline shelf; tiles become draggable.
- `apps/apple/KromaKit/DraggedEffect.swift` — **new.** The `Transferable` payload, so only *our* drags land.
- `apps/apple/PhotoEditor/ContentView.swift` — four cases instead of eight, the Colour tab's five sections, and the picture as a drop target.
- `apps/apple/KromaKitTests/ToolStripTests.swift` → `TabRowTests.swift`; new `DraggedEffectTests.swift`, `InspectorHeaderTests.swift`.

---

### Task 1: Tab and Section replace Tool

**Files:**
- Rename: `crates/pe-effects/src/tool.rs` → `crates/pe-effects/src/tab.rs`
- Modify: `crates/pe-effects/src/lib.rs` (the `mod tool;` line and any `pub use`)
- Test: in `crates/pe-effects/src/tab.rs`'s own test module

**Context:** Read `apps/windows/src/main.rs:1499-1548` before writing this. That match arm is the specification: five `CollapsingHeader`s, in this order, with these default-open states.

| Section | Title Windows uses | Default open | Effect keys |
|---|---|---|---|
| Curves | `Curves` | yes | `curves` |
| Basic | `Basic` | yes | `white_balance`, `exposure`, `contrast`, `tone`, `presence`, `colour` |
| ColourWarper | `Colour Warper` | **no** | `colour_warper` |
| ColourWheels | `Primaries - Color Wheels` | yes | `primaries`, `log_wheels` |
| ColourMixer | `Colour Mixer` | **no** | `colour_mixer` |

Two details that are easy to get wrong and are deliberate on the Windows side. The order is **not** the order the old `Tool::ALL` used — Curves comes first, because "The curve carries the histogram, so there is one rather than two, and it is at the top where a histogram belongs". And the wheels' title is `Primaries - Color Wheels` with an American *Color* and a hyphen, which is Resolve's own label; copy it exactly rather than tidying it.

- [ ] **Step 1: Move the file and write the failing tests**

```bash
git mv crates/pe-effects/src/tool.rs crates/pe-effects/src/tab.rs
```

Replace the contents with `Tab` and `Section`. Write these tests in its test module first:

```rust
    /// Four tabs, in the Windows shell's order. `ALL` is a hand-written list
    /// beside an enum, so the exhaustive match is what fails to compile when a
    /// variant is added and not listed.
    #[test]
    fn all_lists_every_tab() {
        for tab in Tab::ALL {
            match tab {
                Tab::Colour | Tab::Effects | Tab::Image | Tab::File => {}
            }
        }
        assert_eq!(
            Tab::ALL.map(|t| t.name()),
            ["Colour", "Effects", "Image", "File"]
        );
    }

    /// The five sections of the Colour tab, in `main.rs`'s order — which is
    /// not the order they were listed in when each was its own tool. Curves
    /// first, because it carries the histogram.
    #[test]
    fn the_colour_tab_is_five_sections_in_the_windows_order() {
        assert_eq!(
            Section::ALL.map(|s| s.title()),
            [
                "Curves",
                "Basic",
                "Colour Warper",
                "Primaries - Color Wheels",
                "Colour Mixer",
            ]
        );
    }

    /// Two are shut to begin with, and both because they are large and
    /// occasional. A section list that opens everything is the scrolling
    /// problem the tabs exist to solve.
    #[test]
    fn the_warper_and_the_mixer_start_shut() {
        for section in Section::ALL {
            let open = section.starts_open();
            let should = !matches!(section, Section::ColourWarper | Section::ColourMixer);
            assert_eq!(open, should, "{section:?}");
        }
    }

    /// Every pinned effect is drawn by exactly one section. One drawn by none
    /// is a row of the document that appears nowhere, with nothing to say so.
    #[test]
    fn every_pinned_effect_belongs_to_exactly_one_section() {
        for key in crate::PINNED_ROWS {
            let owners: Vec<Section> = Section::ALL
                .into_iter()
                .filter(|s| s.effects().contains(key))
                .collect();
            assert_eq!(owners.len(), 1, "{key} is drawn by {owners:?}");
        }
    }

    /// And no section claims a key that is not a pinned row.
    #[test]
    fn no_section_claims_an_unpinned_effect() {
        for section in Section::ALL {
            for key in section.effects() {
                assert!(
                    crate::PINNED_ROWS.contains(key),
                    "{section:?} claims {key}, which is not pinned"
                );
            }
        }
    }
```

- [ ] **Step 2: Run them to verify they fail**

```bash
cargo test -p pe-effects
```

Expected: FAIL to compile — `Tab` and `Section` do not exist.

- [ ] **Step 3: Write `Tab` and `Section`**

The module doc replaces the old one about a strip of icons. It should say what this file now is: the inspector's four tabs and the five sections of the first of them, mirroring `apps/windows/src/main.rs`, shared rather than written per shell because it is one answer per effect.

```rust
/// One of the inspector's four tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// The grade: five collapsing sections, listed by [`Section::ALL`].
    Colour,
    /// The stack the user built, and the shelf that adds to it.
    Effects,
    /// The crop, the straightening angle, the quarter-turns and the flips.
    /// The one tab that edits the document's *geometry* rather than a row in
    /// its stack.
    Image,
    /// What the photograph is, and what it will be written as.
    File,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Colour, Tab::Effects, Tab::Image, Tab::File];

    pub fn name(self) -> &'static str {
        match self {
            Tab::Colour => "Colour",
            Tab::Effects => "Effects",
            Tab::Image => "Image",
            Tab::File => "File",
        }
    }

    /// Whether the viewer shows the enclosing frame — the whole straightened
    /// source — rather than the cropped result while this tab is open.
    ///
    /// One rule, read twice: it is what puts the crop overlay over the picture
    /// and what is handed to `Session::set_cropping`. A rectangle drawn over a
    /// viewer showing the crop rather than the frame around it has nothing
    /// outside it to drag back in.
    pub fn shows_whole_frame(self) -> bool {
        self == Tab::Image
    }
}

/// One collapsing section of the Colour tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Curves,
    Basic,
    ColourWarper,
    ColourWheels,
    ColourMixer,
}
```

`Section::ALL`, `title()`, `starts_open()` and `effects()` follow, filled from the table above. Give `starts_open` a doc comment saying *why* the warper and the mixer are shut: both are large and occasional, and a list that opens everything is the scrolling problem the tabs exist to solve.

- [ ] **Step 4: Fix the module wiring**

In `crates/pe-effects/src/lib.rs`, `mod tool;` becomes `mod tab;` and any `pub use tool::Tool;` becomes `pub use tab::{Section, Tab};`. Search the workspace for every other use:

```bash
grep -rn "Tool" --include="*.rs" crates/ apps/windows/src/ | grep -v "^crates/pe-effects/src/tab.rs"
```

Fix each. There should be few — the Windows shell has its own `Tab` enum at `main.rs:1754` and does not use `pe_effects::Tool`.

- [ ] **Step 5: Point the Windows shell at the shared `Tab`**

The Windows shell's private `Tab` at `apps/windows/src/main.rs:1754-1772` is now a second copy of the same four. Delete it and use `pe_effects::Tab`, keeping `Tab::ALL` and `Tab::name()` (the shell calls its own method `label`; rename the call sites to `name`). This is the point of moving it: two shells, one list.

If the shell's `Tab` derives something `pe_effects::Tab` does not — check for `Default` and `PartialEq` — add the derive to the shared one rather than keeping the copy.

- [ ] **Step 6: Run the tests and the Windows shell**

```bash
cargo test -p pe-effects && cargo check -p pe-windows
```

Expected: PASS and `Finished`.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "Four tabs, and the five sections of the first"
```

---

### Task 2: The fixture carries both lists

**Files:**
- Modify: `crates/pe-session/tests/fixtures.rs` — the theme fixture's `tools` key
- Regenerate: `apps/apple/Fixtures/theme.json`

**Context:** `theme.json` currently carries `"tools": [{name, effects}]`. It becomes two keys, because there are now two lists and the Swift side mirrors both.

- [ ] **Step 1: Change the generator**

In `crates/pe-session/tests/fixtures.rs`, in the theme fixture test, replace the `tools` block:

```rust
    let tabs: Vec<serde_json::Value> = pe_effects::Tab::ALL
        .iter()
        .map(|t| serde_json::json!({ "name": t.name() }))
        .collect();
    let sections: Vec<serde_json::Value> = pe_effects::Section::ALL
        .iter()
        .map(|s| {
            serde_json::json!({
                "title": s.title(),
                "starts_open": s.starts_open(),
                "effects": s.effects(),
            })
        })
        .collect();
```

and in the `json!` object, `"tools": tools` becomes `"tabs": tabs, "sections": sections`.

- [ ] **Step 2: Regenerate and read the diff**

```bash
PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures the_theme_fixture_is_current && git diff apps/apple/Fixtures/theme.json
```

Expected: the eight `tools` entries replaced by four `tabs` and five `sections`. Check the five titles and the two `"starts_open": false` by eye.

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "The tabs and the sections cross as a fixture"
```

---

### Task 3: The Swift mirrors, and the tab row

**Files:**
- Rename: `apps/apple/KromaKit/Controls/ToolStrip.swift` → `apps/apple/KromaKit/Controls/TabRow.swift`
- Rename: `apps/apple/KromaKitTests/ToolStripTests.swift` → `apps/apple/KromaKitTests/TabRowTests.swift`

**Context:** The icons go. Windows draws four *words* in a row (`tab_row` at `apps/windows/src/main.rs`), not glyphs — read it before drawing this. Four words is also why the SF Symbols reasoning in the old file no longer applies: it existed because eight buttons in a narrow strip could not carry words.

Keep the existing `Drawn` struct and `draws(_:)` logic — they still answer "which rows does this draw", now for `Tab.effects` and `Section`.

- [ ] **Step 1: Write the failing tests**

In `TabRowTests.swift`, keep every test that still applies and rewrite the rest. The fixture test becomes two:

```swift
    /// The four tabs, against the engine's own list.
    func testTheTabsMatchTheEngine() throws {
        let tabs = try XCTUnwrap(fixture()["tabs"] as? [[String: Any]])
        XCTAssertEqual(tabs.count, Tab.allCases.count)
        for (i, entry) in tabs.enumerated() {
            XCTAssertEqual(entry["name"] as? String, Tab.allCases[i].name)
        }
    }

    /// And the five sections of the Colour tab — their titles, their opening
    /// state, and the effects each draws.
    func testTheSectionsMatchTheEngine() throws {
        let sections = try XCTUnwrap(fixture()["sections"] as? [[String: Any]])
        XCTAssertEqual(sections.count, Section.allCases.count)
        for (i, entry) in sections.enumerated() {
            let section = Section.allCases[i]
            XCTAssertEqual(entry["title"] as? String, section.title)
            XCTAssertEqual(entry["starts_open"] as? Bool, section.startsOpen)
            XCTAssertEqual(entry["effects"] as? [String], section.effects)
        }
    }

    /// Every pinned row of a document is drawn by exactly one section, and
    /// every added row by the Effects tab. A row drawn by neither is a row the
    /// user cannot reach and is not told about.
    func testEveryRowIsDrawnExactlyOnce() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixtureData("snapshot"))
        for (index, row) in snap.rows.enumerated() {
            let drawnBySections = Section.allCases
                .filter { $0.draws(snap.rows).contains { $0.index == index } }
            let drawnByEffects = Tab.effects.draws(snap.rows).contains { $0.index == index }
            XCTAssertEqual(
                drawnBySections.count + (drawnByEffects ? 1 : 0), 1,
                "row \(index) (\(row.effect)) is drawn \(drawnBySections.count) times")
        }
    }
```

`fixture()` and `fixtureData(_:)` are stand-ins — reuse the helpers the file already has.

- [ ] **Step 2: Run to verify they fail**

```bash
cd apps/apple && xcodegen generate && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | grep -E "Executed|error:"
```

Expected: compile failure — `Tab` has no `allCases` of four, `Section` does not exist.

- [ ] **Step 3: Write `Tab` and `Section` in Swift**

```swift
/// The inspector's four tabs.
///
/// A mirror of `pe_effects::Tab`, checked against it by `theme.json`. The
/// Windows shell's tab row, word for word: this shell drew eight icons for a
/// while, which put five collapsing sections of one tab up as peers of the
/// other three.
public enum Tab: String, CaseIterable, Sendable {
    case colour = "Colour"
    case effects = "Effects"
    case image = "Image"
    case file = "File"

    public var name: String { rawValue }

    /// Whether the viewer shows the enclosing frame rather than the cropped
    /// result while this tab is open. `pe_effects::Tab::shows_whole_frame`.
    public var showsWholeFrame: Bool { self == .image }
}

/// One collapsing section of the Colour tab.
///
/// A mirror of `pe_effects::Section`. The order is `main.rs`'s and not
/// alphabetical or historical: Curves first, because it carries the histogram
/// and a histogram belongs at the top.
public enum Section: String, CaseIterable, Sendable {
    case curves = "Curves"
    case basic = "Basic"
    case colourWarper = "Colour Warper"
    case colourWheels = "Primaries - Color Wheels"
    case colourMixer = "Colour Mixer"

    public var title: String { rawValue }
    ...
}
```

`Section` carries `effects`, `startsOpen`, `draws(_:)` and `Drawn` — move `draws` and `Drawn` over from the old `Tool` unchanged, and give `Tab.effects` its own `draws` for the added (unpinned) rows.

- [ ] **Step 4: Write `TabRow`**

Four words in a row, in the shape `ToolStrip` already had — `RAISED` behind it, a `RULE` hairline under it, `SELECT` on the chosen one and **not** the accent. Copy that reasoning comment across verbatim; it is still exactly true.

- [ ] **Step 5: Run the suite**

```bash
cd apps/apple && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | grep -E "Executed|error:"
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "Four tabs on the Mac too"
```

---

### Task 4: The inspector draws four tabs

**Files:**
- Modify: `apps/apple/PhotoEditor/ContentView.swift`
- Create: `apps/apple/KromaKit/InspectorHeader.swift`

**Context:** `@AppStorage("tool")` becomes `@AppStorage("tab")` — a *new* key, deliberately. The old key holds names like `"Colour Wheels"` that are no longer tabs, and reusing it would open a build on nothing. A new key means every existing install opens on Colour, which is the right first tab.

Windows puts the photograph's name and its size in an `inspector_header` above the tab row (`main.rs:1492`). The Mac has both in the status bar instead. Windows has them in *both* places, so add the header and leave the status bar alone.

- [ ] **Step 1: Write `InspectorHeader`**

```swift
/// What is being edited, and how big it is.
///
/// `inspector_header` in `main.rs`. Also in the status bar, and deliberately in
/// both: the header says what this column of controls belongs to, and the
/// status bar says what the window is showing. They are the same two facts
/// answering two different questions.
public struct InspectorHeader: View {
    let store: SessionStore
    ...
}
```

The name in `TITLE`, the size in `DIM` beside it, `RAISED` behind, a `Hairline` under. Read `Chrome.swift` for the existing header treatment and match it rather than inventing one.

- [ ] **Step 2: Rewrite the inspector's body**

```swift
    private var inspector: some View {
        VStack(spacing: 0) {
            InspectorHeader(store: store)
            TabRow(chosen: chosenTab)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    switch tab {
                    case .colour:
                        colourSections
                    case .effects:
                        addedRows
                        EffectShelf(...)
                    case .image:
                        CropPanel(store: store)
                    case .file:
                        FilePanel(store: store)
                    }
                }
                .padding(.horizontal, RowMetrics.inset)
            }
        }
        .background(Palette.panel.color)
    }

    /// The Colour tab: five collapsing sections, in the engine's order.
    @ViewBuilder
    private var colourSections: some View {
        ForEach(Section.allCases, id: \.self) { section in
            InspectorSection(
                effect: "colour.\(section.rawValue)",
                title: section.title,
                startsOpen: section.startsOpen
            ) {
                ForEach(section.draws(store.snapshot.rows)) { drawn in
                    if let effect = store.registry.effect(drawn.row.effect) {
                        InspectorPanel(
                            effect: effect, row: drawn.row, store: store,
                            namesTheTool: false
                        )
                    }
                }
            }
        }
    }
```

`InspectorSection` has no `startsOpen` parameter today — its `@AppStorage` is hardcoded to `wrappedValue: true`. Add the parameter, defaulting to `true` so no existing call site changes, and pass it to `AppStorage(wrappedValue:)`.

**`namesTheTool: false`** everywhere here: the section heading now names the effect, so accenting the panel title as well would be the same word twice, and the accent-on-everything problem the strip was built to avoid.

- [ ] **Step 3: Fix the crop wiring**

`store.setCropping(tool.showsWholeFrame)` in `onAppear` and `onChange(of: tool)` become `tab.showsWholeFrame` and `onChange(of: tab)`. Miss either and the viewer shows the cropped picture with nothing outside the rectangle to drag back in — which is the bug the crop port already fixed once.

- [ ] **Step 4: Build and run everything**

```bash
cd apps/apple && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | grep -E "Executed|error:"
```

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "The inspector is four tabs and a header"
```

---

### Task 5: The browser is a shelf

**Files:**
- Modify: `apps/apple/KromaKit/EffectBrowser.swift`

**Context:** `EffectBrowser` is a button that opens a `.popover` (`EffectBrowser.swift:49`). Windows has no popover: the shelf is always visible at the top of the Effects tab, capped at `BROWSER_HEIGHT = 250.0` and scrolling inside that. `inspector.rs:8` says why — "always visible rather than hidden behind a menu … A menu is the right shape for a command you already know the name of. This is a shelf you browse."

A popover also cannot be a drag *source* for Task 6: it is a transient window that closes on outside interaction, and it covers the picture it would be dragged to.

- [ ] **Step 1: Delete the button and the popover**

`EffectBrowser` becomes the shelf itself: the sections, in a `ScrollView`, `.frame(maxHeight: 250)`, with the panel background and a hairline above. The `showing` state goes, and so does the `add:` closure's `showing = false` — there is nothing to close.

Keep `EffectBrowser.sections(in:starred:)` exactly as it is. It is tested and the rules it encodes have not changed.

The disabled-with-nothing-open behaviour stays: `.opacity(store.snapshot.isOpen ? 1 : ScalarRow.dimmed)` and `.disabled(!store.snapshot.isOpen)` move onto the shelf.

- [ ] **Step 2: Give it a heading**

Windows heads it with the shelf inside the Effects tab and the enabled list below. On the Mac the rows come first and the shelf second (`ContentView.swift:157` — "it is directly under the rows it adds to — so an added effect appears where the reader is already looking"). Keep that order; it is a deliberate, stated choice and the tab change does not affect it.

Head the shelf with an `InspectorSection`-style title reading `Add effect`, so it is not an unlabelled wall of tiles now that it is always on screen.

- [ ] **Step 3: Update the tests**

`EffectBrowserTests.swift` — any test asserting the popover or the `showing` state goes. Tests of `sections(in:starred:)` stay untouched.

- [ ] **Step 4: Run the suite and commit**

```bash
cd apps/apple && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | grep -E "Executed|error:"
```

```bash
git add -A && git commit -m "The browser is a shelf, not a menu"
```

---

### Task 6: An effect can be dragged onto the stack or the picture

**Files:**
- Create: `apps/apple/KromaKit/DraggedEffect.swift`
- Create: `apps/apple/KromaKitTests/DraggedEffectTests.swift`
- Modify: `apps/apple/KromaKit/EffectBrowser.swift` (the tile becomes draggable)
- Modify: `apps/apple/PhotoEditor/ContentView.swift` (two drop targets)

**Context:** Windows has exactly two drop targets — the enabled list (`inspector.rs:128`, "The whole panel counts as the target rather than the gap between two rows") and the picture (`main.rs:1608`, "the picture is the larger target — which matters when the thing you are deciding about is what the picture will look like"). Neither chooses a *position*: the drop appends, and the reorder arrows move it afterwards.

**The payload must be our own type.** A bare `String` would make any dragged text from any application add an effect.

- [ ] **Step 1: Write the failing test**

```swift
import UniformTypeIdentifiers
import XCTest
@testable import KromaKit

final class DraggedEffectTests: XCTestCase {
    /// Our own type, not `public.text`. A `String` payload would mean a word
    /// dragged out of any other application adds an effect.
    func testTheTypeIsOurs() {
        XCTAssertEqual(DraggedEffect.type.identifier, "com.kroma.effect-key")
        XCTAssertFalse(DraggedEffect.type.conforms(to: .plainText))
    }

    /// It survives the round trip the drag actually performs.
    func testItRoundTrips() async throws {
        let sent = DraggedEffect(key: "halation")
        let data = try await sent.exported()
        let got = try await DraggedEffect.imported(from: data)
        XCTAssertEqual(got.key, "halation")
    }
}
```

`exported()`/`imported(from:)` are stand-ins — drive `Transferable` through whatever API the SDK offers for a round trip. If none is reachable from a test, assert the `DataRepresentation`'s encode and decode closures directly by exposing them as static functions, and say in a comment that the `Transferable` conformance is a thin wrapper over the two.

- [ ] **Step 2: Write `DraggedEffect`**

```swift
import CoreTransferable
import UniformTypeIdentifiers

/// An effect being dragged out of the shelf.
///
/// Its own uniform type rather than a `String`, so a drop only adds an effect
/// when the thing dropped came from this application's shelf. Text dragged
/// from a browser or an editor is not an effect and must not become one.
public struct DraggedEffect: Codable, Sendable, Transferable {
    public let key: String

    public static let type = UTType(exportedAs: "com.kroma.effect-key")

    public static var transferRepresentation: some TransferRepresentation {
        CodableRepresentation(contentType: type)
    }
}
```

An `exportedAs` type must also be declared in the app's Info.plist under `UTExportedTypeDeclarations`, or macOS will not recognise it. `project.yml` generates the plist — add the declaration there, conforming to `public.data`, and regenerate. If the round-trip test passes without it, keep the declaration anyway: an undeclared type works within one process and fails between two.

- [ ] **Step 3: Make the tile draggable**

In `EffectTile`, after `.onTapGesture(perform: add)`:

```swift
        .draggable(DraggedEffect(key: effect.key)) {
            // What is dragged under the pointer. The tile itself would be a
            // 240-point slab; the name alone is what identifies it.
            Text(effect.name)
                .font(.system(size: 11.5))
                .padding(4)
                .background(Palette.control.color)
        }
```

Click-to-add stays. `.draggable` and `.onTapGesture` coexist: a press that moves is a drag, one that does not is a tap.

- [ ] **Step 4: The stack is a drop target**

In `ContentView`, wrap `addedRows` in a container that takes the drop. The whole list, not the gaps between rows, for the reason `take_drop` gives.

```swift
                    case .effects:
                        VStack(alignment: .leading, spacing: 0) { addedRows }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                            .dropDestination(for: DraggedEffect.self) { dropped, _ in
                                for effect in dropped { store.addEffect(effect.key) }
                                return !dropped.isEmpty
                            }
```

With an empty stack the list has no height and nothing to drop on — give the container a `minHeight` so the target exists before the first effect is added.

- [ ] **Step 5: The picture is a drop target**

On `viewer` in `ContentView`:

```swift
            .dropDestination(for: DraggedEffect.self) { dropped, _ in
                for effect in dropped { store.addEffect(effect.key) }
                return !dropped.isEmpty
            }
```

**Then check the hit-testing.** The crop overlay silently killed zoom, pan and double-click-to-fit once already. A `dropDestination` registers a dragging destination rather than a mouse handler, so it should not take pointer events from `MetalViewerView` — but *should* is not *did*. After this lands, confirm by hand that the wheel still zooms, a drag still pans, and a double-click still fits.

- [ ] **Step 6: Run the suite**

```bash
cd apps/apple && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | grep -E "Executed|error:"
```

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "An effect can be dragged onto the stack or onto the picture"
```

---

### Task 7: Everything green, and looked at

- [ ] **Step 1: The whole workspace**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | tail -3
```

`cargo fmt --all --check` is not optional. CI has failed on it once in this feature already.

- [ ] **Step 2: The Swift suite**

```bash
cd apps/apple && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | grep -E "Executed|error:"
```

- [ ] **Step 3: Build it and look at it**

Copy off the NAS first — nobody runs an application from a network share.

```bash
xcodebuild -project PhotoEditor.xcodeproj -scheme PhotoEditor -configuration Debug -derivedDataPath "$HOME/Kroma build 2/dd" build CODE_SIGNING_ALLOWED=NO
```

Then check, by eye:

1. Four tabs, reading Colour, Effects, Image, File.
2. Colour opens on Curves and Basic and Primaries; the Warper and the Mixer are shut.
3. The header names the photograph and its size.
4. The shelf is always visible in the Effects tab.
5. A tile dragged onto the stack adds it. A tile dragged onto the picture adds it.
6. **The wheel still zooms, a drag still pans, a double-click still fits.**
7. The Image tab still shows the whole frame with the crop rectangle over it.

- [ ] **Step 4: Commit and push**

---

## Notes for whoever executes this

- **Build from a local path, not the NAS mount.** Copy the bundle off `/Volumes/Projects` before running it.
- **`cargo fmt --all --check` before every push.** CI runs it and it has already caught this branch once.
- **One xcodebuild at a time.** Two concurrent runs fail with "database is locked" against the shared DerivedData, which looks like a test failure and is not.
- **The mount flaps.** A `getcwd` EPERM or an unreadable `.git` is the SMB share. Retry.
