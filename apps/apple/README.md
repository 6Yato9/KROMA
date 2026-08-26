# The Apple shells

macOS and iOS/iPadOS, over the same engine Windows uses.

## Building

```bash
brew install xcodegen
cargo install cbindgen
cd apps/apple && xcodegen generate && open PhotoEditor.xcodeproj
```

The tests, without Xcode:

```bash
cd apps/apple && xcodegen generate && xcodebuild test \
  -project PhotoEditor.xcodeproj -scheme KromaKitTests \
  -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO
```

`build-engine.sh` runs as a pre-build phase: it compiles `pe-ffi` for both
Apple architectures, `lipo`s them into a universal static library, and
regenerates `pe_ffi.h`.

## Targets

| | |
|---|---|
| `Spike` | The smallest thing that proves the layer path: a `CAMetalLayer` made in Swift, filled by wgpu in Rust. Kept because it is the fastest way to tell whether a graphics problem is in the engine or in the shell. |
| `PhotoEditor` | The macOS application. Opens a photograph, adds and reorders effects, grades through the pinned panels and anything added to the stack, zooms, undoes, autosaves and exports. Reads the graded frame on waveform, parade, vectorscope and histogram. Draws all eight parameter kinds, including curves and all three of the Colour Warper's views. The fallback for an unknown kind stays, and still says so rather than going quietly missing — a document written by a later version can carry a kind this build has never heard of, and refusing it would stop a photograph opening. |
| `KromaKitTests` | The Swift tests. Compiles `KromaKit/` in as well, under the module name `KromaKit`, so the tests are inside the module they exercise. |

## Why `KromaKit` is a directory and not a Swift package

A Swift package cannot use a bridging header, and the engine arrives as a
generated header plus a static library. Consuming it from SwiftPM would need a
`systemLibrary` target and a hand-written module map pointing at a file that is
generated and gitignored — three moving parts to save nothing. So `KromaKit/`
is a plain directory of sources that XcodeGen compiles into every target that
needs it — the application, the test bundle, and later the iOS app.
`Engine.swift` is the only file in it allowed to touch the C ABI.

## Why the .xcodeproj is generated

A `project.pbxproj` is unmergeable — every branch that adds a file conflicts.
`project.yml` is the source of truth; the Xcode project is a build artefact.

## Fixtures

`Fixtures/` holds `registry.json`, `snapshot.json`, `curve_samples.json`,
`warp_samples.json`, `pin_samples.json`, `scope_graticule.json` and
`backdrop_samples.json`, written by `cargo test -p pe-session --test fixtures` and decoded by
`KromaKitTests`. They are how the two halves of one application are stopped
from drifting apart: add a field in Rust without adding it in Swift, and one of
the two suites fails.

The last four carry more weight than the first two. The curve editor
draws its preview from a **second implementation of the engine's interpolation**,
written in Swift — because asking the engine to bake on every frame of a drag
would put a C call and a 256-float copy inside a gesture. Duplicating an
algorithm is a thing to be uncomfortable about, and this fixture is what makes
it acceptable: eight curves and the engine's output at all 256 LUT positions,
checked against the Swift evaluator at every one of them. Divergence today is
1.85e-07, against a tolerance of 5e-04.

`warp_samples.json` does the same job for the Colour Warper's lattices, whose
axis arithmetic Swift also reimplements: a wrapping axis has `cols` distinct
positions around the ring and never reaches 1.0, an axis with ends has to reach
both. It pins every vertex of six grid sizes on both kinds of axis.

`pin_samples.json` does it for the chromaticity plot, where the trap is a
different one. A pin's `at` and `to` are **CIE xy chromaticities**, not
fractions of the plot — the plot runs −0.03 to 0.88 and `plot_fraction` is the
conversion. `pins.rs` documented them as "0..1" until this was written, and
0.33 read as a fraction lands somewhere entirely different from 0.33 read as a
chromaticity: plausible, and completely wrong.

`scope_graticule.json` does it for the vectorscope's boxes.

`backdrop_samples.json` carries two things at once: which measurement belongs
behind which curve, and the smoothing that turns spiky counts into the curve
they were measured from. The mapping is there because getting it wrong is not
cosmetic — a background counted in the wrong units aims the user at colours the
photograph does not have. `lum_vs_sat` reads an *input luminance* despite its
name leading with saturation, and drew a saturation spread until this was
written down.
`pe_scopes::waveform::position`, `TARGETS` and `SKIN` are `pub` Rust with no C
ABI, so the Swift panel projects the six colour bar targets itself. The fixture
carries each target's position *and* the bin one pixel of that colour actually
lights when it goes through `Vectorscope::from_display` — so a box that ended
up somewhere the pixels cannot reach fails a test rather than looking slightly
wrong on a screen nobody is checking.

So if any of those four ever needs regenerating to make the tests pass, that
is not a fixture that has gone stale — it is the drawn control and the rendered
one having parted company, and one of them is wrong. Find out which before
regenerating.

## One tool at a time

The inspector shows one tool, chosen from an icon strip: Basic, Colour Wheels,
Curves, Colour Warper, Colour Mixer, Effects. Which of the **eleven** pinned
effects each tool draws lives in `pe_effects::Tool` and travels in
`Fixtures/theme.json`, because once the panel shows one tool an effect that
belongs to no tool is drawn nowhere at all — not truncated, not greyed, simply
absent with nothing to say so. Two tests make that impossible, one per side.

Eleven panels in one scrolling column was the arrangement this replaces:
reaching the warper meant scrolling past a hundred and thirty controls.

The strip is drawn with **SF Symbols**, where the Windows shell draws its glyphs
by hand. That is the one place the two shells deliberately differ — a
hand-drawn glyph on macOS would be reproducing a Windows workaround rather than
a design. A missing symbol renders as nothing, so a test asserts every one of
the six resolves.

A selected tool is `SELECT`, not the accent: "this is chosen" and "this is what
you are working in" are different facts. The accent goes on an effect's name
only where the effect *is* the tool — Curves, the Warper, the Mixer. Basic draws
six effects and six accented names is no more use than none.

## The scheme

Every colour comes from `crates/pe-theme`, shared with the Windows shell, and
`Fixtures/theme.json` is what holds the two to it: 27 named colours, which ramp
each parameter's track gets, and what each ramp actually paints at seventeen
steps. Regenerating it to make a test pass means the two shells have parted
company on a colour.

Four greys carry the structure — the viewer surround darkest so nothing
competes with the photograph, a well inside anything read as a graph, a panel
grey for the inspector and scopes, and one step up for headers. One hairline
for every division. And a single warm accent, spent on the name of the effect
you have open and nowhere else: the interface is almost entirely grey, which is
the only reason that one orange title says anything.

`Palette` is a `CaseIterable` enum whose bytes come from an exhaustive switch,
so a colour cannot be added on the Swift side without the fixture test failing.
`PaletteDisciplineTests` additionally greps the sources for `.quaternary`,
`Color(white:)`, `.tint` and friends — crude, but it is what would have caught
the interface being built out of SwiftUI defaults in the first place.

## Measuring

The scopes are counts, not pictures. `pe-scopes` bins a frame and the views
read the numbers, which is what keeps the measuring testable without a display
and what lets it cross a C ABI as plain buffers — a waveform is a `[u32]`.

The session measures on demand, and **any edit throws the measurement away**.
`pe_session_scope_generation` is zero both before the first measurement and
after an edit drops one, so a single number answers both questions a shell has:
is there anything to read, and is it the same as last time. That matters
because a waveform is 2.6 MB and a shell that compared only "has it advanced"
would go on drawing a scope of a photograph that is no longer on screen.

Regenerate deliberately, having looked at the diff:

```bash
PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures
```

## What is deliberately absent

No spectral locus on the chromaticity plot. That is a drawing of the visible
gamut rather than a measurement of the photograph — the Windows shell has a
`locus` module for it and there is no Mac equivalent yet. The plot draws its
frame, its gridlines and the white point, which is enough to place a pin
against.

Everything else that used to be listed here is now drawn: the tone histogram
behind the curve editor, the hue and saturation spreads behind the secondaries,
and the colour cloud on all three of the warper's plots.

No image processing, no colour maths, no shaders — and no workflow rules
either. Where a file may be written, what an export is called and when work in
progress is saved all live in `crates/pe-session`, where Windows uses the same
code and the tests cover it once.
