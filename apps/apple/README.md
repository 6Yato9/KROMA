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
| `PhotoEditor` | The macOS application. Opens a photograph, adds and reorders effects, grades through the pinned panels and anything added to the stack, zooms, undoes, autosaves and exports. Draws six of the eight parameter kinds, curves included. The Colour Warper's lattices and its pins are the two that are not drawn yet; their rows say so rather than going quietly missing. |
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

`Fixtures/` holds `registry.json`, `snapshot.json` and `curve_samples.json`,
written by `cargo test -p pe-session --test fixtures` and decoded by
`KromaKitTests`. They are how the two halves of one application are stopped
from drifting apart: add a field in Rust without adding it in Swift, and one of
the two suites fails.

`curve_samples.json` carries more weight than the other two. The curve editor
draws its preview from a **second implementation of the engine's interpolation**,
written in Swift — because asking the engine to bake on every frame of a drag
would put a C call and a 256-float copy inside a gesture. Duplicating an
algorithm is a thing to be uncomfortable about, and this fixture is what makes
it acceptable: eight curves and the engine's output at all 256 LUT positions,
checked against the Swift evaluator at every one of them. Divergence today is
1.85e-07, against a tolerance of 5e-04.

So if `curve_samples.json` ever needs regenerating to make the tests pass, that
is not a fixture that has gone stale — it is the drawn curve and the rendered
curve having parted company, and one of them is wrong. Find out which before
regenerating.

Regenerate deliberately, having looked at the diff:

```bash
PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures
```

## What is deliberately absent

No histogram behind the curve editor. Resolve draws one, and it needs scope
data that has no C ABI yet — so the editor ships without it rather than with a
decorative one that does not mean anything.

No image processing, no colour maths, no shaders — and no workflow rules
either. Where a file may be written, what an export is called and when work in
progress is saved all live in `crates/pe-session`, where Windows uses the same
code and the tests cover it once.
