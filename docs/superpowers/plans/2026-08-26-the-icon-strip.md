# The Icon Strip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show one tool at a time on macOS, chosen from an icon strip, the way Resolve's colour page works.

**Architecture:** Which effects belong to which tool is shared knowledge — the Windows shell already groups the same nine pinned effects, by hand, in `main.rs`. It moves to `pe-effects` beside the registry so both shells agree and a fixture can check that every pinned effect has exactly one home.

**Tech Stack:** Rust, Swift 6 / SwiftUI, XCTest.

**Predecessors:** the appearance plan, complete. The Mac draws the scheme; it just draws all nine pinned panels at once.

---

## The problem, stated plainly

The macOS inspector stacks nine pinned panels in one scrolling column. Reaching
the Colour Warper means scrolling past a hundred and thirty controls, and every
one of the nine titles wears the accent at the same time — which is the reason
the accent stopped meaning anything even after it was made to follow `open`.

The Windows shell is already better than this: its Colour tab is **five**
collapsing headers, not nine, because it groups the six Lightroom-ish effects
under one "Basic". Two of the five default shut.

Resolve goes further again, and it is what was actually asked for: an icon
strip, and one tool on screen at a time.

## The grouping

Nine pinned effects, six tools — the Windows grouping, plus the added stack:

| tool | effects |
|---|---|
| Basic | `white_balance`, `exposure`, `contrast`, `tone`, `presence`, `colour` |
| Colour Wheels | `primaries`, `log_wheels` |
| Curves | `curves` |
| Colour Warper | `colour_warper` |
| Colour Mixer | `colour_mixer` |
| Effects | everything the user added, and the browser that adds more |

Shared rather than written twice because it is one answer per effect, and
because the Windows copy is currently five hand-written headers in a match arm
— which is exactly the shape that drifts when a tenth pinned effect appears.

## Icons

The Windows shell draws its glyphs by hand, for parity with Resolve's. The Mac
uses **SF Symbols**: they are the platform idiom, they scale with the type, and
they carry accessibility labels for free.

That is a deliberate divergence from the Windows shell and the one place in
this plan where the two are allowed to differ — a hand-drawn glyph on macOS
would be reproducing a Windows workaround rather than a design.

**A symbol that does not exist renders as nothing**, which is a blank button
nobody can identify. `NSImage(systemSymbolName:accessibilityDescription:)`
returns nil for a missing name, so that is a test, and this plan requires it.

---

## Task 1: Which tool an effect belongs to

**Files:**
- Create: `crates/pe-effects/src/tool.rs`
- Modify: `crates/pe-effects/src/lib.rs`, `crates/pe-session/tests/fixtures.rs`

- [ ] **Step 1: Write the failing tests**

```rust
    /// Every pinned effect has exactly one home. A pinned effect belonging to
    /// no tool is one the user cannot reach at all once the panel shows one
    /// tool at a time — it would simply not be drawn, anywhere, with nothing
    /// to say so.
    #[test]
    fn every_pinned_effect_belongs_to_exactly_one_tool() {
        for key in crate::PINNED_ROWS {
            let homes: Vec<Tool> = Tool::ALL
                .iter()
                .copied()
                .filter(|t| t.effects().contains(key))
                .collect();
            assert_eq!(homes.len(), 1, "{key} has {} homes", homes.len());
        }
    }

    /// And no tool claims an effect that is not registered, which is what a
    /// renamed key looks like.
    #[test]
    fn no_tool_claims_an_effect_that_does_not_exist() {
        for tool in Tool::ALL {
            for key in tool.effects() {
                assert!(
                    crate::by_key(key).is_some(),
                    "{tool:?} claims {key}, which is not a registered effect"
                );
            }
        }
    }

    /// The added stack is a tool with no pinned effects of its own — it shows
    /// whatever the user put there.
    #[test]
    fn the_effects_tool_owns_nothing_pinned() {
        assert!(Tool::Effects.effects().is_empty());
    }

    #[test]
    fn the_strip_opens_on_basic() {
        assert_eq!(Tool::ALL.first().copied(), Some(Tool::Basic));
    }
```

- [ ] **Step 2: Write it**

```rust
//! Which tool an effect belongs to.
//!
//! Resolve's colour page shows one tool at a time, chosen from a strip of
//! icons, and nine pinned panels in one scrolling column is the thing that
//! arrangement exists to avoid: reaching the warper means scrolling past a
//! hundred and thirty controls.
//!
//! Shared rather than written per shell because it is one answer per effect.
//! The Windows shell currently spells its own version as five collapsing
//! headers in a match arm, which is the shape that drifts the first time a
//! tenth pinned effect appears.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Basic,
    ColourWheels,
    Curves,
    ColourWarper,
    ColourMixer,
    /// Whatever the user added, and the browser that adds more.
    Effects,
}

impl Tool {
    /// In the order the strip shows them.
    pub const ALL: [Tool; 6] = [
        Tool::Basic,
        Tool::ColourWheels,
        Tool::Curves,
        Tool::ColourWarper,
        Tool::ColourMixer,
        Tool::Effects,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Basic => "Basic",
            Tool::ColourWheels => "Colour Wheels",
            Tool::Curves => "Curves",
            Tool::ColourWarper => "Colour Warper",
            Tool::ColourMixer => "Colour Mixer",
            Tool::Effects => "Effects",
        }
    }

    /// The pinned effects this tool draws, in the order it draws them.
    pub fn effects(self) -> &'static [&'static str] {
        match self {
            Tool::Basic => &[
                "white_balance", "exposure", "contrast", "tone", "presence", "colour",
            ],
            Tool::ColourWheels => &["primaries", "log_wheels"],
            Tool::Curves => &["curves"],
            Tool::ColourWarper => &["colour_warper"],
            Tool::ColourMixer => &["colour_mixer"],
            Tool::Effects => &[],
        }
    }

    /// The tool that draws an effect, if a pinned one does.
    pub fn of(effect: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.effects().contains(&effect))
    }
}
```

Check `PINNED_ROWS` is reachable and spelled as assumed; follow the code if not.

- [ ] **Step 3: Fixture**

Extend `crates/pe-session/tests/fixtures.rs` with a `tools` block: each tool's
name and its effect keys in order. Swift mirrors the enum and asserts against
it, so a tenth pinned effect added on the Rust side and not given a home fails
on both sides rather than silently vanishing from the interface.

- [ ] **Step 4: Verify and commit**

Baseline **675 Rust passed, 0 failed, 1 ignored**. Report the real number.

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures 2>&1 | LC_ALL=C grep -aE "^test result:"; cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | LC_ALL=C grep -aE "^error|^warning"; cargo test --workspace --no-fail-fast 2>&1 | LC_ALL=C grep -aE "^test result:|FAILED"
```

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add -A && git commit -m "Every pinned effect gets exactly one home"
```

---

## Task 2: The strip, and one tool at a time

**Files:**
- Create: `apps/apple/KromaKit/Controls/ToolStrip.swift`
- Modify: `apps/apple/PhotoEditor/ContentView.swift`
- Create: `apps/apple/KromaKitTests/ToolStripTests.swift`

- [ ] **Step 1: Write the failing tests**

```swift
    /// A symbol that does not exist renders as nothing, and a blank button in a
    /// strip of six is one nobody can identify. `NSImage` returns nil for a name
    /// the system does not have, so this is checkable rather than a hope.
    func testEveryToolHasASymbolTheSystemActuallyHas() {
        for tool in Tool.allCases {
            XCTAssertNotNil(
                NSImage(systemSymbolName: tool.symbol, accessibilityDescription: nil),
                "\(tool.name) asks for \(tool.symbol), which this system does not have")
        }
    }

    func testTheToolsAndTheirEffectsMatchTheEngine() throws {
        let tools = try XCTUnwrap(fixture()["tools"] as? [[String: Any]])
        XCTAssertEqual(tools.count, Tool.allCases.count)
        for (i, entry) in tools.enumerated() {
            let tool = Tool.allCases[i]
            XCTAssertEqual(entry["name"] as? String, tool.name, "tool \(i)")
            XCTAssertEqual(entry["effects"] as? [String], tool.effects, tool.name)
        }
    }

    /// Every pinned effect the registry declares is reachable from some tool.
    /// One that is not would simply not be drawn anywhere, with nothing to say
    /// so — the worst kind of missing control.
    func testEveryPinnedEffectIsReachable() throws {
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        let owned = Set(Tool.allCases.flatMap(\.effects))
        for row in snap.rows where row.pinned {
            XCTAssertTrue(
                owned.contains(row.effect),
                "\(row.effect) is pinned but no tool draws it")
        }
    }

    /// Each tool names what it will show, so a strip button is not a guess.
    func testEveryToolHasAnAccessibilityLabel() {
        for tool in Tool.allCases {
            XCTAssertFalse(tool.name.isEmpty)
        }
    }
```

- [ ] **Step 2: Write the strip**

`ToolStrip` is a row of six buttons: an SF Symbol each, the selected one on
`SELECT` with `TITLE` ink, the rest `ICON` on the panel. A `.help()` tooltip
carrying the name, and the name as the accessibility label. `RAISED` behind the
strip and a `RULE` hairline under it, because it is a header.

Symbols — check each resolves before settling on it, and say in your report
which you used and whether any first choice had to be replaced:

| tool | first choice |
|---|---|
| Basic | `slider.horizontal.3` |
| Colour Wheels | `circle.lefthalf.filled` |
| Curves | `point.topleft.down.curvedto.point.bottomright.up` |
| Colour Warper | `square.grid.3x3` |
| Colour Mixer | `paintpalette` |
| Effects | `wand.and.stars` |

**The accent does not go on the strip.** A selected tool is "this is chosen",
which is `SELECT`; the accent stays on the name of the effect you are working
in. That distinction is the whole reason both colours exist.

- [ ] **Step 3: Show one tool**

`ContentView`'s inspector currently walks every row. It now:

1. draws the strip,
2. for a pinned tool, draws only the panels for `tool.effects`, in that order,
3. for `Tool.Effects`, draws the added rows and the browser as it does today.

The selected tool is remembered in `@AppStorage`.

**Effect panels keep their fold**, and with one tool on screen the accent is
finally spent on one or two names rather than nine. Do not remove the folding
to compensate; the two solve different problems.

**Nothing may become unreachable.** The `EffectBrowser` currently sits under
every panel; it belongs to `Tool.Effects` now. Check that adding an effect still
works and that the added row appears where the user is looking.

- [ ] **Step 4: Verify and commit**

Baseline **196 Swift tests, 0 failures**. Report the real number, and build the
app: `** BUILD SUCCEEDED **`, no new Swift warnings. Two are pre-existing:
cargo's `block v0.1.6` note and the AppIntents metadata note.

Existing tests render panels directly and should not care, but
`PaletteDisciplineTests` scans the sources — a new file must obey it.

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add apps/apple && git commit -m "One tool at a time, chosen from a strip"
```

---

## Task 3: Look at it

`caffeinate -u -t 600 &` **before launching** — the display sleeping is what
makes `screencapture` return black, and launching while it sleeps defers the
window so `System Events` reports none.

```bash
open ~/Library/Developer/Xcode/DerivedData/PhotoEditor-*/Build/Products/Debug/PhotoEditor.app
osascript -e 'delay 6' -e 'tell application "PhotoEditor" to activate' -e 'tell application "System Events" to tell process "PhotoEditor" to set position of window 1 to {0, 40}' -e 'tell application "System Events" to tell process "PhotoEditor" to set size of window 1 to {1500, 940}'
screencapture -T 2 -x -R0,40,1500,940 /tmp/kroma.png
```

Capture **each of the six tools** — `osascript -e 'tell application "System Events" to click at {x, y}'` presses a strip button. Read every image.

Check: does the strip read as a strip; is any icon ambiguous at its drawn size;
is the accent now spent on one thing; does each tool's panel fill the space
sensibly or leave it empty; can you still reach everything.

Fix what you find, capture again, and report what still looks wrong.

---

## Task 4: Write it down

`apps/apple/README.md`: the inspector shows one tool at a time, the grouping
lives in `pe-effects` and is shared, and the Mac uses SF Symbols where the
Windows shell draws its glyphs — with the reason.

Then the whole tree, and commit.

---

## Verification

| check | command | expected |
|---|---|---|
| Rust | `cargo test --workspace --no-fail-fast` | 0 failed |
| Swift | `xcodebuild test -scheme KromaKitTests` | 0 failed |
| format / lint | `cargo fmt --all --check`, `clippy -D warnings` | silent |
| app | `xcodebuild build -scheme PhotoEditor` | BUILD SUCCEEDED, no warnings |
| eye | Task 3's six captures | read, and reported on |

`every_pinned_effect_belongs_to_exactly_one_tool` and
`testEveryPinnedEffectIsReachable` are the pair that matter. Showing one tool at
a time means an effect with no tool is drawn nowhere at all, and nothing else in
the suite would notice.
