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
| `PhotoEditor` | The macOS application. Opens a photograph, adds and reorders effects, grades through the pinned panels and anything added to the stack, zooms, undoes, autosaves and exports. Draws all eight parameter kinds, including curves and all three of the Colour Warper's views. The fallback for an unknown kind stays, and still says so rather than going quietly missing — a document written by a later version can carry a kind this build has never heard of, and refusing it would stop a photograph opening. |
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
`warp_samples.json` and `pin_samples.json`, written by `cargo test -p pe-session --test fixtures` and decoded by
`KromaKitTests`. They are how the two halves of one application are stopped
from drifting apart: add a field in Rust without adding it in Swift, and one of
the two suites fails.

The last three carry more weight than the first two. The curve editor
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

So if any of those three ever needs regenerating to make the tests pass, that
is not a fixture that has gone stale — it is the drawn control and the rendered
one having parted company, and one of them is wrong. Find out which before
regenerating.

Regenerate deliberately, having looked at the diff:

```bash
PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures
```

## What is deliberately absent

No histogram behind the curve editor, no colour distribution behind a warper
lattice, and no spectral locus on the chromaticity plot — that last draws its
frame, its gridlines and the white point, which is enough to place a pin
against. Resolve draws both, and the Windows shell composites the second
over the space itself as a haze showing where this photograph's colours
actually fall. Both need scope data, which has no C ABI yet — so they ship
without rather than with a decorative version that does not mean anything. The
lattice does draw the space it sits over, because a grid on a black square says
nothing at all about which colours it is moving.

No image processing, no colour maths, no shaders — and no workflow rules
either. Where a file may be written, what an export is called and when work in
progress is saved all live in `crates/pe-session`, where Windows uses the same
code and the tests cover it once.
