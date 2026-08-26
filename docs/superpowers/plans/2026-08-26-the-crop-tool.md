# The Crop Tool Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Crop, straighten, rotate and flip on macOS — the largest editing feature the Windows shell has and the Mac does not.

**Architecture:** Different from every port before it. `pe_core::Geometry` already holds the whole model *and* all of its arithmetic — `shrink_to_fit`, `slide_to_fit`, `apply_aspect`, `corners`, `enclosing`. So Swift reimplements **nothing**: it proposes a geometry, the engine corrects it to something legal, and hands the corrected value straight back through out-parameters. One C call per drag frame, no snapshot decode, no second copy of the maths to keep honest.

**Tech Stack:** Rust, the `pe-ffi` C ABI (two functions), Swift 6 / SwiftUI, XCTest.

**Predecessors:** the icon strip. Crop becomes a seventh tool.

---

## Why this one needs no fixture

The curve editor, the warper lattices, the pins and the theme all ended with a
committed fixture, because each duplicated arithmetic across the language
boundary and the fixture was what stopped the copies drifting.

There is nothing to duplicate here. `Geometry` is seven primitive fields:

```rust
pub struct Geometry {
    pub centre: [f32; 2],   // offset from the middle, in source widths
    pub size: [f32; 2],     // fraction of the source
    pub angle: f32,         // degrees, positive anticlockwise
    pub turns: u8,          // quarter-turns clockwise, 0..3
    pub flip_h: bool,
    pub flip_v: bool,
    pub aspect: AspectLock,
}
```

and every rule about what makes one *legal* — that the crop stays inside the
straightened source, that a locked aspect is honoured, that dragging a corner
past the frame slides rather than shrinks — already lives on it, in Rust, with
tests. Reimplementing `shrink_to_fit` in Swift to avoid a round trip would be
inventing exactly the drift the last five plans worked to prevent, to save a
call that costs nothing.

**So the shape is: Swift proposes, the engine corrects, the corrected value
comes straight back.** That is what the out-parameters in Task 2 are for, and
it is why this plan has no `apps/apple/Fixtures/*.json` in it.

## What the tool is

From `apps/windows/src/crop.rs`, which is the reference:

- **The overlay.** While the tool is open the viewer shows the *enclosing*
  frame — the whole source, straightened — rather than the cropped result, so
  you can see what you are cutting away and drag back into it. Eight grips and
  the region itself; the area outside the crop is dimmed.
- **The panel.** Aspect lock, an angle slider, quarter-turn buttons, two flips,
  and a reset.

Both hang off `Geometry`, so the panel and the overlay cannot disagree.

---

## Task 1: The document's geometry crosses

**Files:**
- Modify: `crates/pe-session/src/describe.rs`, `session.rs`
- Modify: `crates/pe-session/tests/fixtures.rs` (the snapshot gains a field)

- [ ] **Step 1: Write the failing tests**

`Geometry` is not in the snapshot at all — `grep -c geometry crates/pe-session/src/describe.rs` returns 0 — so Swift cannot see the current crop. Put it there, and give the session a setter that corrects what it is given.

```rust
    #[test]
    fn a_fresh_document_has_no_crop_and_says_so() {
        let s = chart_session();
        let g = s.geometry().expect("something is open");
        assert!(g.is_identity(), "a fresh photograph is not cropped");
    }

    /// The engine corrects what it is handed. A crop that hangs off the edge is
    /// not a crop anyone can render, and the shell should not have to know the
    /// rules to avoid proposing one — that is what `shrink_to_fit` and
    /// `slide_to_fit` are for, and they live here.
    #[test]
    fn a_crop_that_hangs_off_the_edge_is_brought_back_inside() {
        let mut s = chart_session();
        let mut want = pe_core::Geometry::default();
        want.centre = [0.9, 0.9];
        want.size = [0.5, 0.5];
        let got = s.set_geometry(want).unwrap();
        assert!(
            got.fits(1024, 768),
            "the engine returned a crop that is not inside the source: {got:?}"
        );
        assert_ne!(got.centre, want.centre, "nothing was corrected");
    }

    /// And the corrected value is what the document holds — the caller is told
    /// the truth rather than being left holding what it asked for.
    #[test]
    fn what_comes_back_is_what_was_stored() {
        let mut s = chart_session();
        let mut want = pe_core::Geometry::default();
        want.angle = 12.0;
        want.size = [0.4, 0.4];
        let got = s.set_geometry(want).unwrap();
        assert_eq!(s.geometry().unwrap(), got);
    }

    #[test]
    fn a_locked_aspect_is_honoured() {
        let mut s = chart_session();
        let mut want = pe_core::Geometry::default();
        want.aspect = pe_core::AspectLock::Ratio(1.0);
        want.size = [0.8, 0.4];
        let got = s.set_geometry(want).unwrap();
        let (w, h) = got.output_size(1024, 768);
        assert!(
            (w as f32 / h as f32 - 1.0).abs() < 0.02,
            "a square lock produced {w}x{h}"
        );
    }

    #[test]
    fn setting_a_geometry_with_nothing_open_is_refused() {
        let mut s = Session::new();
        assert!(s.set_geometry(pe_core::Geometry::default()).is_err());
        assert!(s.geometry().is_none());
    }

    /// The snapshot carries it, so the shell can draw the crop it is editing.
    #[test]
    fn the_snapshot_carries_the_geometry() {
        let mut s = chart_session();
        let mut want = pe_core::Geometry::default();
        want.angle = 7.5;
        want.turns = 1;
        want.flip_h = true;
        s.set_geometry(want).unwrap();
        let json = serde_json::to_value(s.describe()).unwrap();
        let g = &json["geometry"];
        assert!((g["angle"].as_f64().unwrap() - 7.5).abs() < 1e-4);
        assert_eq!(g["turns"], 1);
        assert_eq!(g["flip_h"], true);
    }
```

Check `s.describe()` is what produces the snapshot and follow its actual
spelling; `AspectLock` is an enum and needs a wire form Swift can read — look
at how `ParamValue::Choice` does it and pick something as simple.

- [ ] **Step 2: Write it**

`Session::geometry()` returning `Option<Geometry>`, and:

```rust
    /// Set the crop, straighten and flips, and return what was actually stored.
    ///
    /// The engine corrects: a crop is brought inside the straightened source
    /// and a locked aspect is honoured. The caller is handed the corrected
    /// value rather than the one it asked for, because the alternative is a
    /// shell drawing a rectangle the renderer will not produce — and because
    /// the rules live on `Geometry`, where they are tested, rather than in
    /// each shell.
    pub fn set_geometry(&mut self, want: Geometry) -> Result<Geometry, SessionError>
```

The correction order matters and `crop.rs` establishes it: apply the aspect,
then bring it inside. Read `apply_aspect`, `shrink_to_fit` and `slide_to_fit`
and follow what the Windows tool does rather than inventing an order.

Adding a field to the snapshot changes `apps/apple/Fixtures/snapshot.json`;
regenerate it and confirm from the diff that **only** the geometry block is new.

- [ ] **Step 3: Verify and commit**

Baseline **684 Rust passed, 0 failed, 1 ignored**. Report the real number.

The Swift suite will fail here if a test decodes the snapshot strictly — check,
and if so say so; Task 3 is where it is fixed.

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add -A && git commit -m "The crop is something the shell can see"
```

---

## Task 2: Propose, correct, return

**Files:**
- Modify: `crates/pe-ffi/src/lib.rs`

- [ ] **Step 1: The two entry points**

```rust
/// Set the crop, straighten and flips, and write back what was actually
/// stored.
///
/// Every out-pointer may be null. The engine corrects what it is given — a
/// crop is brought inside the straightened source and a locked aspect is
/// honoured — so the values written back are frequently *not* the ones passed
/// in, and a caller that ignores them will draw a rectangle the renderer does
/// not produce.
///
/// Nine in, seven out, all primitives: this is a drag path, and a JSON parse
/// per frame to carry seven numbers is work nobody needs done.
///
/// # Safety
/// `s` must be valid or null; each non-null out-pointer must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_geometry(
    s: *mut PeSession,
    cx: f32, cy: f32,
    w: f32, h: f32,
    angle: f32,
    turns: u32,
    flip_h: bool,
    flip_v: bool,
    aspect: f32,          // <= 0 means free
    out_cx: *mut f32, out_cy: *mut f32,
    out_w: *mut f32, out_h: *mut f32,
    out_angle: *mut f32,
    out_turns: *mut u32,
    out_aspect: *mut f32,
) -> i32
```

and `pe_session_reset_geometry(s) -> i32`, which is `set_geometry(default)` —
its own function because "back to the original" is a thing a user asks for
directly and a shell should not have to construct a default to say it.

`aspect` as a float with `<= 0` meaning free keeps the ABI to primitives.
Document it on both sides; a magic value nobody wrote down is worse than an
enum.

- [ ] **Step 2: Tests**

Following `a_vertex_crosses_the_boundary_and_a_bad_one_is_refused`:

```rust
    #[test]
    fn a_geometry_crosses_and_comes_back_corrected() {
        // Ask for a crop hanging off the corner; the values written back must
        // be inside the source, and must differ from what was asked for.
    }

    #[test]
    fn a_null_out_pointer_is_allowed() {
        // A caller that does not care what was stored passes nulls and gets a
        // status code.
    }

    #[test]
    fn resetting_puts_it_back_to_the_whole_frame() {
    }
```

Fill each in against the real snapshot, as the warp and pin tests do. Assert on
the *document*, not only the return code — a status alone proves nothing.

- [ ] **Step 3: Verify and commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add crates && git commit -m "Swift proposes a crop and the engine says what it really is"
```

---

## Task 3: Swift reads and sets it

**Files:**
- Modify: `apps/apple/KromaKit/Snapshot.swift`, `Engine.swift`, `SessionStore.swift`
- Modify: `apps/apple/KromaKitTests/SnapshotTests.swift`, `EngineTests.swift`

- [ ] **Step 1: Decode**

`GeometryValue` mirroring the seven fields, with `isIdentity`. Decoded from the
snapshot's new `geometry` block. The existing snapshot tests must still pass —
if one decodes strictly and now fails, that is Task 1's change arriving, and
the fix is here.

- [ ] **Step 2: Set**

`Session.setGeometry(_:) throws -> GeometryValue` — **returns the corrected
value**, which is the whole point. The doc comment must say so, because a call
site that discards it will draw a crop the engine did not accept.

`SessionStore.setGeometry` holds the corrected value for the overlay to draw
and skips the snapshot refresh mid-drag, like every other drag path here.

- [ ] **Step 3: Tests**

```swift
    func testACropThatHangsOffTheEdgeComesBackInside()
    func testTheReturnedGeometryIsWhatTheDocumentHolds()
    func testResettingRestoresTheWholeFrame()
```

- [ ] **Step 4: Verify and commit**

---

## Task 4: The overlay

**Files:**
- Create: `apps/apple/KromaKit/Controls/CropOverlay.swift`
- Modify: `apps/apple/KromaKit/MetalViewer.swift` or `PhotoEditor/ContentView.swift`

- [ ] **Step 1: Draw it**

While the tool is open the viewer shows the **enclosing** frame — the whole
source, straightened — rather than the cropped result. That is what makes the
tool usable: you can see what you are cutting away and drag back into it.
`Geometry::enclosing` gives that frame and is already shared.

Eight grips (four corners, four edges) plus the region itself for panning.
Outside the crop is dimmed. A thirds grid while dragging.

Each drag: work out the proposed geometry, call `store.setGeometry`, and draw
**what came back**. Never draw the proposal — the engine may have corrected it,
and drawing the proposal is a rectangle that jumps when the drag ends.

- [ ] **Step 2: Test what a render can show**

The grips and the dimming are appearance; use the `ImageRenderer` approach that
`RowMetricsTests`, `CurveBackdropTests` and `WarperCloudTests` established, and
**prove each test discriminates by breaking the thing it names.**

Worth pinning: the region outside the crop is dimmer than inside; a corner grip
sits on a corner; dragging a corner past the frame does not move the rectangle
outside it.

- [ ] **Step 3: Verify and commit**

---

## Task 5: The panel, and an eighth tool

**Files:**
- Create: `apps/apple/KromaKit/Controls/CropPanel.swift`
- Modify: `apps/apple/KromaKit/Controls/ToolStrip.swift`, `PhotoEditor/ContentView.swift`
- Modify: `crates/pe-effects/src/tool.rs`

- [ ] **Step 1: A tool that owns no effects**

`Tool.Effects` already draws no pinned effects; Crop is the second such tool —
it edits the document's geometry rather than a row in the stack. Add it to
`pe_effects::Tool` with an empty `effects()`, and mirror it in Swift. The
existing test `every_pinned_effect_belongs_to_exactly_one_tool` still holds;
`the_effects_tool_owns_nothing_pinned` needs a sibling or a rename.

Symbol: `crop` resolves on macOS 14 — **verify it, do not assume**, the way
`ToolStripTests` already does for the other six.

- [ ] **Step 2: The panel**

Aspect lock (Free, Original, and the usual ratios), the angle as a `ScalarRow`
so it looks like every other row, four quarter-turn and flip buttons, and a
reset. All through `store.setGeometry`, all drawing the corrected value.

- [ ] **Step 3: Verify and commit**

---

## Task 6: Look at it, then write it down

- [ ] **Step 1** — `caffeinate -u -t 600 &` **before launching**; a sleeping
display returns a black frame and, if it sleeps before launch, defers the
window so `System Events` reports none. Capture the tool open, a corner drag,
and a straighten. Read the images.

- [ ] **Step 2** — `apps/apple/README.md`: crop is the seventh tool, the engine
corrects what the shell proposes, and there is no fixture here because there is
no duplicated arithmetic to hold together — say that, because every other port
in this repository has one and its absence should look deliberate.

---

## Verification

| check | command | expected |
|---|---|---|
| Rust | `cargo test --workspace --no-fail-fast` | 0 failed |
| Swift | `xcodebuild test -scheme KromaKitTests` | 0 failed |
| non-Apple | `cargo check --workspace --all-targets --exclude pe-windows --target x86_64-unknown-linux-gnu` | clean |
| format / lint | `cargo fmt --all --check`, `clippy -D warnings` | silent |
| app | `xcodebuild build -scheme PhotoEditor` | BUILD SUCCEEDED, no warnings |

The non-Apple check is new to this plan's list and belongs in every one after
it: the engine stopped compiling on Windows for about a hundred commits and
nothing local noticed, because the only thing that would have was a CI matrix
aimed at a branch this repository has never had.

`a_crop_that_hangs_off_the_edge_is_brought_back_inside` is the test that
matters. It is the whole architecture in one assertion: the shell may propose
anything, and what it gets back is something the renderer will actually draw.
