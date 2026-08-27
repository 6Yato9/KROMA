# The Spectral Locus Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Draw the horseshoe on the Colour Warper's chromaticity plot, so it is a map of colour rather than a square with a cloud on it.

**Architecture:** `apps/windows/src/locus.rs` says it in its own header — *data plus a couple of geometric questions, and both are testable without a screen*. It moves to `pe-color`, where the colour science lives, and stops hardcoding a matrix that crate already derives.

**Tech Stack:** Rust, Swift 6 / SwiftUI, XCTest.

**Predecessors:** compare. This is the smallest of what is left and the last piece of the warper.

---

## What it is

> The horseshoe is the boundary of colour itself: every real colour is a mixture
> of spectral lights, so every real colour sits inside the curve those lights
> trace on the CIE diagram. A chromaticity plot that draws a triangle instead is
> drawing one display's gamut and calling it the world.

Four things, all pure:

- `LOCUS` — the CIE 1931 2° spectral locus in xy, 33 points at 10 nm from 380 to
  700 nm. The polygon closes from 700 back to 380, which is the line of purples:
  colours that are real and have no wavelength.
- `curve()` — the same, subdivided with Catmull-Rom for drawing.
- `inside(x, y)` — is this a colour that exists.
- `colour_at(x, y)` — what it looks like, as near as a display can put it,
  answered for the **whole plane** so the caller can dim what is outside rather
  than leaving it black.

## The matrix it should not be carrying

`locus.rs` hardcodes `XYZ_TO_SRGB` as the published constants. `pe-color`
already derives exactly that from `primaries::SRGB` via `Primaries::rgb_to_xyz`,
so the moved module should **derive it and drop the literal** — one fewer place
for a matrix to drift.

Do not simply delete the numbers. Turn them into a test: the derived matrix must
agree with the published one to a small tolerance. That check is worth more than
the constant was, because it validates `pe-color`'s own derivation against the
standard rather than assuming it.

## Catmull-Rom, deliberately

The curve editor rejected Catmull-Rom because it overshoots between control
points and a tone curve that bulges is a bright halo nobody asked for. A
spectral locus is a smooth closed curve and overshoot there is invisible;
`curve()` uses it on purpose. Do not "fix" it to match the other one.

---

## Task 1: The locus moves to the colour crate

**Files:**
- Create: `crates/pe-color/src/locus.rs`
- Modify: `crates/pe-color/src/lib.rs`, `apps/windows/src/locus.rs`, `warper.rs`
- Modify: `crates/pe-session/tests/fixtures.rs`
- Create: `apps/apple/Fixtures/locus.json`

- [x] **Step 1: Move it**

Every doc comment comes too — they carry the reasoning about why the whole
plane is answered, why the clip is towards white rather than per channel ("a
plot whose greens turn cyan at the edge is worse than one whose greens go
pale"), and why it is normalised to full brightness rather than by luminance.

Replace `XYZ_TO_SRGB` with a derivation from `primaries::SRGB`, and keep the
published numbers as the test described above.

`apps/windows/src/locus.rs` becomes a re-export, or its two call sites in
`warper.rs` point at `pe_color::locus` — whichever leaves less behind. Say which.

- [x] **Step 2: The tests it already has, plus the ones it does not**

It has an anchors test; keep it. Add:

```rust
    /// The derived matrix agrees with the published constants.
    #[test]
    fn the_derived_matrix_is_the_published_one() { }

    /// Inside is inside and outside is outside, at points that are not close.
    #[test]
    fn a_real_colour_is_inside_and_an_impossible_one_is_not() { }

    /// The line of purples closes the polygon: a colour below the ends and
    /// between them in x is real, and one below the line is not.
    #[test]
    fn the_line_of_purples_closes_the_horseshoe() { }

    /// Answered for the whole plane, so a plot can dim rather than blacken.
    #[test]
    fn a_colour_outside_the_horseshoe_still_has_something_to_draw() { }

    /// Except where the arithmetic has nothing to say.
    #[test]
    fn there_is_no_colour_at_no_luminance() { }

    /// Clipped towards white, so a green at the edge goes pale rather than cyan.
    #[test]
    fn an_out_of_gamut_green_goes_pale_rather_than_changing_hue() { }
```

That last one is the interesting assertion: take a chromaticity well outside
sRGB's triangle but inside the horseshoe, and check the drawn colour's **hue**
is close to what a per-channel clip would have shifted it away from.

- [x] **Step 3: The fixture**

`curve()` sampled, `inside` at a set of probes, and `colour_at` at the same
probes. Swift asserts against it, as every other reimplementation here does.

- [x] **Step 4: Verify and commit**

Baseline **777 Rust passed, 0 failed, 1 ignored**. Report the real number.
Run `cargo check --workspace --all-targets --exclude pe-windows --target x86_64-unknown-linux-gnu` and `cargo clippy --workspace --all-targets`.

---

## Task 2: The Mac draws it

**Files:**
- Create: `apps/apple/KromaKit/Controls/Locus.swift`
- Modify: `apps/apple/KromaKit/Controls/PinsEditor.swift`
- Create: `apps/apple/KromaKitTests/LocusTests.swift`

- [x] **Step 1: Mirror and check**

`Locus.curve`, `Locus.inside(_:_:)`, `Locus.colour(at:)`, all asserted against
`locus.json`. Exact equality where the arithmetic allows it; say where it does
not and why.

- [x] **Step 2: Draw it**

The chromaticity plot in `PinsEditor` currently draws its frame, its gridlines
and the white point. It gains the field: **the whole square coloured, dimmed
outside the horseshoe**, the curve stroked over it, and the existing gridlines
and white point on top.

The dimming is the point, and `plot_image`'s comment is the argument: a black
surround makes the plot a shape floating in nothing; a dimmed one makes it a
bright region of a continuous field, which is what a gamut actually is.

Build it as a `CGImage` once, not as sixteen thousand rectangles — the same
reason `WarperCloud` and the scopes do.

`PinGeometry` already maps chromaticity to the plot; use it rather than a second
mapping.

- [x] **Step 3: Test what a render can show**

Use the `ImageRenderer` approach and **prove each test discriminates**. Worth
pinning: a point inside the horseshoe is brighter than one outside; the greenest
corner is green; the plot is not uniform.

---

## Task 3: Write it down

`apps/apple/README.md` currently says the chromaticity plot draws no spectral
locus and calls that deliberate. It is no longer true — rewrite it, and say what
the locus is for.

---

## Verification

| check | command | expected |
|---|---|---|
| Rust | `cargo test --workspace --no-fail-fast` | 0 failed |
| Swift | `xcodebuild test -scheme KromaKitTests` | 0 failed |
| non-Apple | `cargo check --workspace --all-targets --exclude pe-windows --target x86_64-unknown-linux-gnu` | clean |
| Windows shell | `cargo clippy --workspace --all-targets` | clean |
| app | `xcodebuild build -scheme PhotoEditor` | BUILD SUCCEEDED, no warnings |

`the_derived_matrix_is_the_published_one` is the one that earns its place: it
turns a constant that was going to drift into a check on the crate that derives
it.
