#!/usr/bin/env bash
# Build the Rust engine as a universal static library for the Mac app, and
# regenerate the C header Swift imports.
#
# Run from the repo root or from apps/macos; Xcode runs it as a build phase.
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

cargo build -p pe-ffi --target aarch64-apple-darwin $PROFILE_FLAG
cargo build -p pe-ffi --target x86_64-apple-darwin  $PROFILE_FLAG

mkdir -p "target/universal/$PROFILE_DIR"
lipo -create \
  "target/aarch64-apple-darwin/$PROFILE_DIR/libpe_ffi.a" \
  "target/x86_64-apple-darwin/$PROFILE_DIR/libpe_ffi.a" \
  -output "target/universal/$PROFILE_DIR/libpe_ffi.a"

# The header is generated, never hand-edited, so it cannot drift from the Rust.
if command -v cbindgen >/dev/null 2>&1; then
  cbindgen --config cbindgen.toml --crate pe-ffi \
           --output apps/macos/PhotoEditor/pe_ffi.h
else
  echo "warning: cbindgen not installed; run 'cargo install cbindgen'" >&2
fi

echo "built target/universal/$PROFILE_DIR/libpe_ffi.a"
