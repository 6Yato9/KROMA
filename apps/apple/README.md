# The Apple shells

macOS and iOS/iPadOS, over the same engine Windows uses.

## Building

```bash
brew install xcodegen
cargo install cbindgen
cd apps/apple && xcodegen generate && open PhotoEditor.xcodeproj
```

`build-engine.sh` runs as a pre-build phase: it compiles `pe-ffi` for both
Apple architectures, `lipo`s them into a universal static library, and
regenerates `pe_ffi.h`.

## Targets

| | |
|---|---|
| `Spike` | The smallest thing that proves the layer path: a `CAMetalLayer` made in Swift, filled by wgpu in Rust. Kept because it is the fastest way to tell whether a graphics problem is in the engine or in the shell. |
| `PhotoEditor` | Where the macOS application goes. Still the M0 scaffold: it links against the engine and round-trips a document, and nothing more. |

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
