#!/usr/bin/env bash
# Build the Rust engine as a universal static library for the Mac app, and
# regenerate the C header Swift imports.
#
# Run from the repo root or from apps/apple; Xcode runs it as a build phase.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

CONFIG="${CONFIGURATION:-Debug}"
PROFILE_FLAG=""
PROFILE_DIR="debug"
if [ "$CONFIG" != "Debug" ]; then
  PROFILE_FLAG="--release"
  PROFILE_DIR="release"
fi

rustup target add aarch64-apple-darwin x86_64-apple-darwin

# Where cargo puts the objects, which is not necessarily inside the repository.
# A checkout on a network share is the case that forces this: incremental
# compilation cannot lock there, and a half-written artefact fails to load with
# an error that blames the dependency rather than the disc.
BUILD_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

cargo build -p pe-ffi --target aarch64-apple-darwin $PROFILE_FLAG
cargo build -p pe-ffi --target x86_64-apple-darwin  $PROFILE_FLAG

# The universal library lands inside the repository wherever the objects were
# built, because `LIBRARY_SEARCH_PATHS` in project.yml has to name a path that
# is the same on every machine. It is a few megabytes; the objects are gigabytes.
mkdir -p "$ROOT/target/universal/$PROFILE_DIR"
lipo -create \
  "$BUILD_DIR/aarch64-apple-darwin/$PROFILE_DIR/libpe_ffi.a" \
  "$BUILD_DIR/x86_64-apple-darwin/$PROFILE_DIR/libpe_ffi.a" \
  -output "$ROOT/target/universal/$PROFILE_DIR/libpe_ffi.a"

# Drop the debug info. Swift never steps into Rust — the engine's tests live on
# the Rust side — and leaving it in makes XCTest's symbolication of a *failing*
# test read the whole archive, which on a network share does not finish. A test
# that hangs instead of reporting its failure is worse than the failure.
strip -S "$ROOT/target/universal/$PROFILE_DIR/libpe_ffi.a" 2>/dev/null || true

# The header is generated, never hand-edited, so it cannot drift from the Rust.
if command -v cbindgen >/dev/null 2>&1; then
  cbindgen --config cbindgen.toml --crate pe-ffi \
           --output apps/apple/PhotoEditor/pe_ffi.h
  cp apps/apple/PhotoEditor/pe_ffi.h apps/apple/Spike/pe_ffi.h
else
  echo "warning: cbindgen not installed; run 'cargo install cbindgen'" >&2
fi

echo "built target/universal/$PROFILE_DIR/libpe_ffi.a"
