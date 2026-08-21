# macOS app

Scaffolded at M0, built at M6. It exists now so that the engine/UI firewall is
real from the first commit rather than aspirational: `crates/pe-ffi` has a
consumer, and `Engine.swift` is the only file on this side that may touch it.

## Building, on a Mac

```bash
brew install xcodegen cbindgen
cd apps/macos
xcodegen generate
open PhotoEditor.xcodeproj
```

`build-engine.sh` runs as a pre-build phase: it compiles `pe-ffi` for both
Apple architectures, `lipo`s them into a universal static library, and
regenerates `pe_ffi.h`.

## Why the .xcodeproj is generated

A `project.pbxproj` is unmergeable — every branch that adds a file conflicts.
`project.yml` is the source of truth; the Xcode project is a build artefact.

## What is deliberately absent

No image processing, no colour maths, no shaders. If any of that appears in
this directory, the port has gone wrong: it all belongs in `crates/`, where it
is shared with Windows and covered by the golden tests.
