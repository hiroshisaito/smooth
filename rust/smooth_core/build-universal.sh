#!/usr/bin/env bash
# Build libsmooth_core.a as a universal (x86_64 + arm64) static library.
# Invoked by Xcode Run Script phase (Mac) or manually.
set -euo pipefail

CRATE_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$CRATE_DIR"

cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin

OUT_DIR="$CRATE_DIR/target/universal/release"
mkdir -p "$OUT_DIR"

lipo -create \
    "$CRATE_DIR/target/x86_64-apple-darwin/release/libsmooth_core.a" \
    "$CRATE_DIR/target/aarch64-apple-darwin/release/libsmooth_core.a" \
    -output "$OUT_DIR/libsmooth_core.a"

echo "[smooth_core] universal .a -> $OUT_DIR/libsmooth_core.a"
lipo -info "$OUT_DIR/libsmooth_core.a"
