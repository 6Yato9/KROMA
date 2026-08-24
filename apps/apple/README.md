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
| `PhotoEditor` | Where the macOS application goes. It compiles `KromaKit/` in and links against the engine; the interface itself is still a placeholder. |
| `KromaKitTests` | The Swift tests. Compiles `KromaKit/` in as well, under the module name `KromaKit`, so the tests are inside the module they exercise. |

## Why `KromaKit` is a directory and not a Swift package

A Swift package cannot have a bridging header, and the engine arrives as a
generated header plus a static library. So `KromaKit/` is a plain directory of
sources that XcodeGen compiles into every target that needs it — the
application, the test bundle, and later the iOS app. `Engine.swift` is the only
file in it allowed to touch the C ABI.

## Why the .xcodeproj is generated

A `project.pbxproj` is unmergeable — every branch that adds a file conflicts.
`project.yml` is the source of truth; the Xcode project is a build artefact.

## Fixtures

`Fixtures/` holds `registry.json` and `snapshot.json`, written by
`cargo test -p pe-session --test fixtures`. They are committed so that the
Swift tests can decode them once those exist, which is how the two halves of
one application will be stopped from drifting apart: add a field in Rust
without adding it in Swift and one of the two suites fails. Until then the
Rust half of that check already works — the committed copy is compared against
what the code produces now, so the registry cannot change without somebody
noticing.

Regenerate deliberately, having looked at the diff:

```bash
PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures
```

## What is deliberately absent

No image processing, no colour maths, no shaders — and no workflow rules
either. Where a file may be written, what an export is called and when work in
progress is saved all live in `crates/pe-session`, where Windows uses the same
code and the tests cover it once.
