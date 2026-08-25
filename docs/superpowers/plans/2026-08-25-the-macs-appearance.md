# The Mac Shell's Appearance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the macOS shell look like Kroma rather than like a SwiftUI default, by sharing the palette and the track ramps the Windows shell already has.

**Architecture:** The scheme is Resolve's, read off its colour page, and it already exists — `apps/windows/src/theme.rs`. It is in a binary crate, so Swift cannot reach it and a fixture cannot generate from it. It moves to a new `pe-theme` crate, exactly as `Warp::home`, the plot range and `trace` moved before it.

**Tech Stack:** Rust, Swift 6 / SwiftUI, XCTest.

**Predecessors:** everything through the backgrounds plan. The Mac app draws all eight parameter kinds, the scopes, and the backdrops — in stock SwiftUI greys and the system accent blue.

---

## What is wrong, specifically

Having finally looked at it running: the macOS shell draws the right *things* in the wrong *clothes*. Every colour is a SwiftUI default — `.quaternary` for a track, `.tint` (system blue) for a fill, `.primary` for a handle — so nothing shares a palette with anything and the whole panel reads as an unstyled form.

Beside the Windows shell it is missing, concretely:

| | Windows | macOS today |
|---|---|---|
| surfaces | four greys: viewer 18, well 22, panel 33, raised 43 | one flat background |
| track | 4pt bar, grey 74, filled from **neutral** in 122 | hairline, filled from the left in system blue |
| neutral mark | a tick where the parameter does nothing | none |
| handle | a **pointer**, point up, dark-outlined | a plain circle |
| value | a **boxed number**, draggable at ¼ speed, typeable | static text |
| section | a rule, a chevron, collapsible | small grey text |
| ramps | blue→yellow on temperature, the hue circle on hue, … | none |
| accent | one warm salmon, spent only on what is *active* | system blue, everywhere |

The last row is the one that matters most. Resolve's interface is almost
entirely grey, which is why its single orange title tells you where you are
without having to shout. An accent on every control says nothing at all.

## Why a shared crate rather than a second palette

Two palettes drift. The Windows file says so in its own header — before it
existed the greys were written per call site and the viewer surround, the
filmstrip and the status bar had already become three different shades of one
colour. A second copy in Swift is the same mistake at a larger scale.

`pe-theme` holds the numbers and the ramps and depends on nothing. Each shell
keeps only its own glue: egui `Visuals` on one side, SwiftUI `Color` on the
other.

## The handle is not a detail

The Windows pointer is a house shape, point up, one pixel wide where it meets
the track, with a dark outline. That is not styling: a disc covers the part of
the gradient you are trying to read, and its widest part sits exactly where you
want to see the colour underneath. On a hue ramp a circular handle hides the
hue it is pointing at. The outline is there because the fill is light grey and
would vanish against the pale end of a temperature or luma ramp.

## Scope

The palette, the ramps, the parameter row, section headers, and the surfaces.
**Not** a redesign — this is the existing Windows scheme applied to the Mac,
and anywhere the two could differ the Windows one wins.

---

## Task 1: The palette moves somewhere both shells can reach

**Files:**
- Create: `crates/pe-theme/{Cargo.toml,src/lib.rs,src/ramp.rs}`
- Modify: `apps/windows/src/theme.rs`, `Cargo.toml`
- Modify: `crates/pe-session/tests/fixtures.rs`
- Create: `apps/apple/Fixtures/theme.json`

- [x] **Step 1: Make the crate**

Move out of `apps/windows/src/theme.rs`, unchanged in behaviour:

- the whole `colour` module — every constant, with its comment
- `Ramp`, `Ramp::at`, `Ramp::is_plain`
- `ramp_for`, `band_hue`, `CHANNEL_AXES`
- `hsv`, `mix3`, `lerp`, and whatever else those need

`pe-theme` must **not** depend on egui. Colours become a plain
`#[repr(C)] pub struct Rgb8 { pub r: u8, pub g: u8, pub b: u8 }` with a `const fn new`.
The Windows shell keeps a one-line `fn c(Rgb8) -> egui::Color32` and its
`apply(ctx)`, and re-exports the rest so its own call sites do not all change:

```rust
pub use pe_theme::{Ramp, ramp_for};
```

Keep every doc comment. They carry the reasoning — why the surround is darker
than the picture, why the accent is spent on so little, why the ramps are
hand-picked rather than converted through a linear HSV — and that reasoning is
the part a second shell most needs.

- [x] **Step 2: Prove nothing moved**

The Windows shell must be byte-identical in behaviour. It has an ignored test
that writes plots to files and a suite of ordinary ones:

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && cargo test --workspace --no-fail-fast 2>&1 | LC_ALL=C grep -aE "^test result:|FAILED"
```

Baseline **666 passed, 0 failed**. A pure move should leave it at 666 plus
whatever tests move with the code.

Add to `pe-theme` the tests that pin the parts a second implementation gets
wrong:

```rust
    /// The ramp table is matched on whole words. `contains("tint")` looked fine
    /// until a `tilt` or a `saturation` inside some unrelated effect picked up a
    /// gradient that made a promise the control does not keep.
    #[test]
    fn a_ramp_is_matched_on_the_whole_key() {
        assert_eq!(ramp_for("white_balance", "temperature"), Ramp::Temp);
        assert_eq!(ramp_for("anything", "tint"), Ramp::Tint);
        assert!(ramp_for("anything", "tilt").is_plain());
        assert!(ramp_for("anything", "desaturation_amount").is_plain());
    }

    #[test]
    fn a_mixer_band_gets_its_own_hue_window() {
        assert_eq!(ramp_for("colour_mixer", "red_hue"), Ramp::HueAround(0.0));
        assert!(matches!(ramp_for("colour_mixer", "green_saturation"), Ramp::Sat(_)));
        assert_eq!(ramp_for("colour_mixer", "blue_luminance"), Ramp::Luma);
        // A band nobody named is not a band.
        assert!(ramp_for("colour_mixer", "beige_hue").is_plain());
    }

    /// A saturation ramp does not get brighter as it gets more colourful — a
    /// saturation control does not change how bright the picture is, and a ramp
    /// that said it did would be lying about the parameter.
    #[test]
    fn the_chroma_ramp_holds_its_lightness() {
        let ends = [Ramp::Chroma.at(0.15), Ramp::Chroma.at(0.85)];
        let luma = |c: Rgb8| 0.2126 * c.r as f32 + 0.7152 * c.g as f32 + 0.0722 * c.b as f32;
        assert!(
            (luma(ends[0]) - luma(ends[1])).abs() < 60.0,
            "the chroma ramp changes lightness across its span: {:?}",
            ends
        );
    }

    #[test]
    fn every_ramp_is_defined_across_its_whole_span_and_clamps_outside() {
        for ramp in [
            Ramp::Plain, Ramp::Temp, Ramp::Tint, Ramp::Hue, Ramp::HueAround(120.0),
            Ramp::Sat(Rgb8::new(200, 80, 80)), Ramp::Chroma, Ramp::Luma,
            Ramp::Axis(Rgb8::new(0, 200, 200), Rgb8::new(200, 0, 0)),
        ] {
            for t in [-1.0_f32, 0.0, 0.5, 1.0, 2.0] {
                let _ = ramp.at(t);
            }
            assert_eq!(ramp.at(-1.0), ramp.at(0.0), "{ramp:?} does not clamp low");
            assert_eq!(ramp.at(2.0), ramp.at(1.0), "{ramp:?} does not clamp high");
        }
    }
```

`Ramp` needs `Debug` for those messages.

- [x] **Step 3: Write the fixture**

Add to `crates/pe-session/tests/fixtures.rs`:

```rust
/// The palette and the track ramps.
///
/// Both shells draw from one set of numbers. Before this crate existed the
/// Windows greys were written at each call site and had already drifted — the
/// viewer surround, the filmstrip and the status bar were three shades of what
/// was meant to be one colour. A second copy in Swift is that mistake again,
/// so the numbers cross here and the Swift side asserts against them.
#[test]
fn the_theme_fixture_is_current() {
    use pe_theme::{colour, ramp_for, Ramp, Rgb8};

    let hex = |c: Rgb8| format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b);

    let mut palette = serde_json::Map::new();
    for (name, c) in colour::ALL {
        palette.insert(name.to_string(), serde_json::json!(hex(*c)));
    }

    // Which ramp each parameter of each registered effect gets, so a key
    // renamed on one side and not the other is caught.
    let mut ramps = serde_json::Map::new();
    for effect in pe_effects::all() {
        for p in effect.params {
            let r = ramp_for(effect.key, p.key);
            if !r.is_plain() {
                ramps.insert(
                    format!("{}.{}", effect.key, p.key),
                    serde_json::json!(format!("{r:?}")),
                );
            }
        }
    }

    // And what each ramp actually paints, sampled — a table that agrees on
    // *which* ramp and disagrees on its colours is no use.
    let mut sampled = serde_json::Map::new();
    for (name, ramp) in [
        ("Temp", Ramp::Temp), ("Tint", Ramp::Tint), ("Hue", Ramp::Hue),
        ("Chroma", Ramp::Chroma), ("Luma", Ramp::Luma),
        ("HueAround(28)", Ramp::HueAround(28.0)),
    ] {
        let steps: Vec<String> = (0..=16)
            .map(|i| hex(ramp.at(i as f32 / 16.0)))
            .collect();
        sampled.insert(name.to_string(), serde_json::json!(steps));
    }

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "palette": palette,
        "ramps": ramps,
        "sampled": sampled,
    }))
    .unwrap();
    check("theme.json", json);
}
```

`colour::ALL` does not exist — add it to `pe-theme` as
`pub const ALL: &[(&str, &Rgb8)]`, listing every constant. **A palette entry
missing from `ALL` never reaches Swift**, so add a test that the count matches
the number of `pub const` colours, or build `ALL` with a macro that declares
them — whichever you can make genuinely hard to forget. Say which you did.

- [x] **Step 4: Verify and commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures 2>&1 | LC_ALL=C grep -aE "^test result:"; cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | LC_ALL=C grep -aE "^error|^warning"; cargo test --workspace --no-fail-fast 2>&1 | LC_ALL=C grep -aE "^test result:|FAILED"
```

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add -A && git commit -m "The scheme moves somewhere both shells can read it"
```

---

## Task 2: Swift draws from the same numbers

**Files:**
- Create: `apps/apple/KromaKit/Palette.swift`, `Controls/Ramp.swift`
- Create: `apps/apple/KromaKitTests/PaletteTests.swift`

- [x] **Step 1: Write the failing tests**

```swift
    /// Every colour in the application comes from one place, and that place is
    /// shared with the Windows shell. A palette that drifts is how a viewer
    /// surround, a filmstrip and a status bar become three shades of one grey.
    func testEveryPaletteEntryMatchesTheEngine() throws {
        let palette = try XCTUnwrap(fixture()["palette"] as? [String: String])
        for (name, hex) in palette {
            let colour = try XCTUnwrap(
                Palette.named(name), "Swift has no colour called \(name)")
            XCTAssertEqual(
                Palette.hex(colour), hex,
                "\(name) is \(Palette.hex(colour)) here and \(hex) in the engine")
        }
    }

    /// And nothing extra, which is the half that catches a colour invented on
    /// the Swift side and then quietly used in three places.
    func testSwiftHasNoColoursTheEngineDoesNot() throws {
        let palette = try XCTUnwrap(fixture()["palette"] as? [String: String])
        XCTAssertEqual(
            Set(Palette.allNames), Set(palette.keys),
            "the two palettes name different colours")
    }

    func testEveryRampChoiceMatchesTheEngine() throws {
        let ramps = try XCTUnwrap(fixture()["ramps"] as? [String: String])
        for (path, want) in ramps {
            let parts = path.split(separator: ".", maxSplits: 1)
            let got = Ramp.for(effect: String(parts[0]), key: String(parts[1]))
            XCTAssertEqual(String(describing: got), want, path)
        }
    }

    func testEveryRampPaintsWhatTheEngineWouldPaint() throws {
        let sampled = try XCTUnwrap(fixture()["sampled"] as? [String: [String]])
        for (name, steps) in sampled {
            let ramp = try XCTUnwrap(Ramp.named(name), "no Swift ramp called \(name)")
            for (i, want) in steps.enumerated() {
                let got = Palette.hex(ramp.at(Double(i) / 16))
                XCTAssertEqual(got, want, "\(name) at step \(i)")
            }
        }
    }
```

- [x] **Step 2: Write them**

`Palette.swift` holds the constants as `Color` **and** as the raw bytes, since
the test compares hex — build them from one `(r, g, b)` triple each so the two
cannot disagree. `Palette.named(_:)` and `allNames` exist for the test and are
the reason a colour cannot be added on the Swift side alone.

`Ramp.swift` mirrors `Ramp` and `ramp_for`, including `hsv` — note the Windows
comment on why it is hand-rolled rather than converted through a linear HSV, and
reproduce **that** conversion, not a different one that looks close.

Exact colour equality is the assertion, so any rounding difference between the
two `hsv` implementations fails the test rather than being absorbed. If you
cannot make them agree to the byte, report the largest difference and where —
do not switch the test to a tolerance without saying so.

- [x] **Step 3: Verify and commit**

Baseline **165 Swift tests**. Report the real number.

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add apps/apple && git commit -m "One palette, and Swift is held to it"
```

---

## Task 3: The row, as Resolve draws it

**Files:**
- Modify: `apps/apple/KromaKit/ParameterRow.swift`
- Modify: `apps/apple/KromaKitTests/RowMetricsTests.swift`

- [x] **Step 1: Rebuild `ScalarRow` on the palette**

Four changes, each with a reason worth keeping in a comment:

1. **The track** is a 4pt rounded bar in `TRACK`, filled from **neutral** to the
   value in `TRACK_FILL` — not from the left. On a bipolar control the fill
   growing out of the middle is the only drawing that gives the sign at a
   glance.
2. **The neutral mark**, a 9pt tick in `HANDLE_EDGE` where the parameter does
   nothing — drawn only when neutral sits between 4% and 96% along, because at
   either end the track's own end is already the mark.
3. **The handle** is the pointer, not a circle: a house shape, point up, 10pt
   wide, 12.5pt tall, filled `HANDLE` (`HANDLE_HOT` when hovered or dragged),
   outlined 1pt in `HANDLE_EDGE`. A disc covers the gradient it points at.
4. **The value** is a boxed number: `BOX_FILL` inside, 1pt `BOX_EDGE`, 17pt
   tall inside a 22pt row — a field that fills its row reads as a button, and
   thirty stacked up is a wall of boxes rather than a column of numbers.

Label text is `LABEL` at 11.5pt, right-aligned. A disabled row dims everything
it draws by 0.42 — SwiftUI's `.disabled` will not do it, because everything
here is painted with a colour of its own.

- [x] **Step 2: The box is a control, not a readout**

Dragging it changes the value **four times more slowly than the track**, and it
accepts typing. That ratio is the point of having both: the track is for
finding roughly the right value, the box for settling on one, and a box that
moved at the same rate would just be a second slider.

Test the ratio directly:

```swift
    func testTheBoxIsFinerThanTheTrack() {
        // The same drag, on each. The box must move the value less.
        let bounds = Bounds(min: -1, max: 1, default: 0, neutral: 0)
        let byTrack = ScalarRow.valueDragging(bounds: bounds, from: 0, by: 40, over: 200)
        let byBox = ScalarRow.valueTyping(bounds: bounds, from: 0, by: 40)
        XCTAssertGreaterThan(abs(byTrack), abs(byBox) * 2, "the box is not finer")
    }
```

Spell those two helpers however the implementation wants; the assertion is
that one is meaningfully finer than the other.

- [x] **Step 3: Ramps**

`FloatRow` asks `Ramp.for(effect:key:)` and hands the result to `ScalarRow`.
A ramped track draws the gradient instead of the grey bar and **no fill** —
the gradient is already showing the axis, and a fill over it would hide the
part being pointed at.

`ScalarRow` is also used by the pin controls, which have no registry key; they
pass `.plain` and keep the grey bar.

- [x] **Step 4: Test what a render can show**

Extend `RowMetricsTests` with the `ImageRenderer` approach already there:

```swift
    /// A bipolar control fills out of the middle, so the sign is readable
    /// without the number.
    func testABipolarFillGrowsFromTheMiddle()

    /// And a unipolar one from its own end.
    func testAUnipolarFillGrowsFromTheLeft()

    /// The pointer does not cover what it points at: on a hue ramp, the colour
    /// immediately under the handle's tip must still be that hue.
    func testTheHandleDoesNotHideTheGradientBeneathIt()

    /// A disabled row is dimmer than an enabled one, everywhere.
    func testADisabledRowIsDimmedThroughout()
```

**Verify each of these fails when you break the thing it names** — fill from
the left, a circular handle, no dimming — and say in your report which
mutations you checked. That discipline is what the last four plans established
and it is the only thing making a headless assertion about appearance worth
anything.

- [x] **Step 5: Verify and commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add apps/apple && git commit -m "The row wears the scheme, and its handle stops hiding the ramp"
```

---

## Task 4: Sections, surfaces, and the rest of the controls

**Files:**
- Modify: `apps/apple/KromaKit/InspectorPanel.swift`, `PhotoEditor/ContentView.swift`
- Modify: `apps/apple/KromaKit/Controls/{BoolRow,ChoiceRow,RgbRow,WheelView}.swift`
- Modify: `StackRowView.swift`, `EffectBrowser.swift`, `Controls/ScopeViews.swift`

- [x] **Step 1: Section headers**

The registry gives every parameter a `section`, and the Mac inspector currently
ignores it except where the warper claims one — which is why the panel is one
long undifferentiated column. Draw a header per section: a `RULE` hairline
along the top, a chevron, and the title in `TITLE` at 12pt. **Collapsible**,
remembering its state, because thirty parameters in one column is the reason
Resolve made them collapse.

The effect's own name is the one thing drawn in `ACCENT` — Resolve titles the
open effect in it and spends it nowhere else. Resist using it for anything
here; an accent on every heading says nothing.

- [x] **Step 2: Surfaces**

Four greys, doing the job they were chosen for:

- `VIEWER` behind the photograph. Darkest, so nothing competes with the frame —
  a surround lighter than the picture's own shadows makes the shadows look
  lifted, which is a lie told to someone grading them.
- `PANEL` for the inspector, the scopes panel and the status bar. Today these
  are three different SwiftUI defaults.
- `WELL` inside anything read as a graph: the curve editor, the warper plots,
  the scope wells.
- `RAISED` for headers and the toolbar.

Divisions are the single `RULE` hairline, not `Divider()`'s default.

- [x] **Step 3: The other four control kinds**

`BoolRow`, `ChoiceRow`, `RgbRow` and `WheelView` are on SwiftUI defaults and
will now be the only things that are. Put each on the palette, keeping the row
geometry they already share. A choice's popup and a checkbox both need
`CONTROL` / `CONTROL_HOT` rather than the system control colour.

The `SELECT` blue is for "this is chosen"; `ACCENT` is for "this is doing
something". They are different facts and Resolve keeps them apart — the scopes
panel's toggles currently use the accent for selection, which is the wrong one.

- [x] **Step 4: Verify and commit**

Every existing test must still pass — this is appearance, and the geometry
tests, the fixture checks and the backdrop tests all assert things that must not
move. If one breaks, the change went further than appearance; say which and
why.

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add apps/apple && git commit -m "Four greys, one hairline, and the accent spent on one thing"
```

---

## Task 5: Look at it

This is the task that was impossible until today. `screencapture` works; the
earlier black frames were a sleeping display, not a permission.

- [x] **Step 1: Build, launch, and capture**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild build -project PhotoEditor.xcodeproj -scheme PhotoEditor -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | LC_ALL=C grep -aE "error:|BUILD"
```

Then:

```bash
caffeinate -u -t 600 &
open ~/Library/Developer/Xcode/DerivedData/PhotoEditor-*/Build/Products/Debug/PhotoEditor.app
osascript -e 'delay 5' -e 'tell application "PhotoEditor" to activate' -e 'tell application "System Events" to tell process "PhotoEditor" to set position of window 1 to {0, 40}' -e 'tell application "System Events" to tell process "PhotoEditor" to set size of window 1 to {1500, 940}'
screencapture -T 2 -x -R0,40,1500,940 /tmp/kroma.png
```

**`caffeinate -u` is not optional.** The display sleeps during a build and
`screencapture` then returns a black frame, which is exactly what made this
unverifiable for weeks.

To scroll the inspector, build the tiny event poster — AppleScript cannot send
a scroll wheel and neither `cliclick` nor Python's Quartz bindings are on this
machine:

```swift
// scroll.swift — swiftc -O scroll.swift -o scroll && ./scroll <x> <y> <clicks>
import CoreGraphics
import Foundation
let a = CommandLine.arguments
let at = CGPoint(x: Double(a[1])!, y: Double(a[2])!)
let n = Int32(a[3])!
CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: at,
        mouseButton: .left)?.post(tap: .cghidEventTap)
usleep(120_000)
for _ in 0..<abs(n) {
    let e = CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1,
                    wheel1: n > 0 ? -3 : 3, wheel2: 0, wheel3: 0)
    e?.location = at
    e?.post(tap: .cghidEventTap)
    usleep(30_000)
}
```

`osascript -e 'tell application "System Events" to click at {x, y}'` clicks, and
`set size of window 1` resizes. Both work; the accessibility permission is
already granted.

- [x] **Step 2: Actually look, and write down what is wrong**

Capture the inspector top, a section further down, the curve editor, and each
of the warper's three tabs. **Read each image.** For every one, write down what
is wrong before fixing anything — the point of finally being able to see it is
to find the things no test was ever going to state.

Things to check against the Windows shell, which can be run beside it:

- do the four greys read as four, or has one collapsed into another
- is the accent anywhere it should not be
- do the ramps look like the parameter, or like decoration
- does the pointer read as pointing, at the size it is actually drawn
- is the boxed number legible at 17pt, or too tight
- does a section header read as a division, or as another row

- [x] **Step 3: Fix what you found, and capture again**

Iterate until the panel reads as one thing. Keep the before and after.

- [x] **Step 4: Report honestly**

Say what you changed, what still looks wrong and why you left it, and what you
could not judge. A claim that something "looks right" now has to be backed by
an image you actually read.

---

## Task 6: Write it down

- [x] **Step 1** — `apps/apple/README.md` gains a note that the scheme lives in
`pe-theme` and is shared with the Windows shell, with the `theme.json` fixture
alongside the others and the same warning: regenerating it to make a test pass
means the two shells have parted company on a colour.

- [x] **Step 2** — `apps/windows/src/theme.rs` is now glue. Its header explains
the scheme; move that explanation to `pe-theme`'s and leave a pointer, so the
reasoning sits with the numbers rather than with one shell's adapter.

- [x] **Step 3: Verify the whole tree and commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | LC_ALL=C grep -aE "^error|^warning"; cargo test --workspace --no-fail-fast 2>&1 | LC_ALL=C grep -aE "^test result: FAILED|^error"; echo "rust done"
```

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add -A && git commit -m "The scheme, written down where it lives"
```

---

## Verification

| check | command | expected |
|---|---|---|
| Rust | `cargo test --workspace --no-fail-fast` | 0 failed |
| Swift | `xcodebuild test -scheme KromaKitTests` | 0 failed |
| format | `cargo fmt --all --check` | silent |
| lint | `cargo clippy --workspace --all-targets -- -D warnings` | silent |
| app | `xcodebuild build -scheme PhotoEditor` | BUILD SUCCEEDED, no warnings |
| eye | Task 5's captures | read, and reported on |

`testEveryPaletteEntryMatchesTheEngine` and `testSwiftHasNoColoursTheEngineDoesNot`
are the pair that matter: one stops Swift drifting from the scheme, the other
stops Swift quietly inventing a colour outside it. Everything else here is
drawing, and drawing is what Task 5 is for.
