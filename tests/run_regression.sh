#!/usr/bin/env bash
# Phase 2-A.2 Step 3: manifest-driven regression runner.
#
# Reads tests/goldens/<suite>/manifest.toml, verifies fixture integrity, then
# runs `regression_test` over each frame the manifest enumerates. Replaces
# the earlier glob-driven runner so that adding/removing/replacing a frame is
# a manifest edit instead of a "drop a .raw file in the directory" operation.
#
# Tolerance policy (NEAR-ID for cross-platform / for the frame-135 boundary
# residual) currently still lives in regression_test.cpp's hardcoded
# `diff < 0.01% && max_abs <= 32` rule. The manifest already describes the
# same rule under `cross_platform_policy` + frame 135's `policy_overrides`;
# Step 4+ harness work will move enforcement into manifest reads.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="$ROOT/references/AfterEffectsSDK_25.6_61_win/ae25.6_61.64bit.AfterEffectsSDK"
GOLDENS_ROOT="$ROOT/tests/goldens"
BIN="$ROOT/tests/regression_test"
WHITE_BIN="$ROOT/tests/test_white_option"
RUST_CRATE="$ROOT/rust/smooth_core"
RUST_LIB="$RUST_CRATE/target/universal/release/libsmooth_core.a"

# Suites to run: explicit args override default of all suites with a manifest.
if [ "$#" -gt 0 ]; then
    SUITES=("$@")
else
    SUITES=()
    while IFS= read -r m; do
        SUITES+=("$(basename "$(dirname "$m")")")
    done < <(find "$GOLDENS_ROOT" -mindepth 2 -maxdepth 2 -name manifest.toml | sort)
fi

if [ "${#SUITES[@]}" -eq 0 ]; then
    echo "no manifest.toml found under $GOLDENS_ROOT" >&2
    exit 1
fi

echo "==> verify goldens integrity (per-suite SHA256)"
"$ROOT/tests/fetch_goldens.sh" "${SUITES[@]}" || {
    echo "fetch_goldens.sh failed; aborting regression"
    exit 1
}

echo "==> build Rust smooth_core (universal)"
"$RUST_CRATE/build-universal.sh" >/dev/null || { echo "cargo build failed"; exit 1; }

SMOOTH_PARALLEL="${SMOOTH_PARALLEL:-1}"

# Phase 2-A.3 C-2.5a: libsmooth_core.a now references Objective-C runtime
# and Metal framework symbols on macOS (smooth_core_metal_* FFI). The CPU
# regression harnesses don't call those symbols, but the linker still needs
# them resolved. -lobjc + Foundation/Metal/QuartzCore covers what metal-rs
# pulls in transitively. On non-Apple platforms these are unset and the
# compiler picks them up from the host SDK.
case "$(uname -s)" in
  Darwin)
    GPU_LINK_FLAGS="-lobjc -framework Foundation -framework Metal -framework QuartzCore"
    ;;
  *)
    GPU_LINK_FLAGS=""
    ;;
esac

echo "==> build regression harness (SMOOTH_PARALLEL=$SMOOTH_PARALLEL)"
clang++ -std=c++17 -O2 \
    -DSMOOTH_PARALLEL=$SMOOTH_PARALLEL \
    -I"$SDK/Examples/Headers" \
    -I"$SDK/Examples/Headers/SP" \
    -I"$SDK/Examples/Util" \
    -I"$RUST_CRATE/include" \
    -I"$ROOT" \
    "$ROOT/tests/regression_test.cpp" \
    "$ROOT/util.cpp" \
    "$RUST_LIB" \
    $GPU_LINK_FLAGS \
    -o "$BIN" || { echo "build failed"; exit 1; }

echo "==> build synthetic white_option harness"
clang++ -std=c++17 -O2 \
    -DSMOOTH_PARALLEL=$SMOOTH_PARALLEL \
    -I"$SDK/Examples/Headers" \
    -I"$SDK/Examples/Headers/SP" \
    -I"$SDK/Examples/Util" \
    -I"$RUST_CRATE/include" \
    -I"$ROOT" \
    "$ROOT/tests/test_white_option.cpp" \
    "$ROOT/util.cpp" \
    "$RUST_LIB" \
    $GPU_LINK_FLAGS \
    -o "$WHITE_BIN" || { echo "white-option build failed"; exit 1; }

echo "==> run synthetic white_option tests"
"$WHITE_BIN" || { echo "white_option regression FAILED"; exit 1; }

# Enumerate (in_path, out_path) pairs from each manifest. Output one
# tab-separated line per frame so we can iterate from the shell without
# re-parsing TOML in bash.
list_frames() {
    local manifest="$1"
    local suite_dir="$2"
    python3 - "$manifest" "$suite_dir" <<'PY'
import os, sys, tomllib
manifest_path, suite_dir = sys.argv[1], sys.argv[2]
with open(manifest_path, "rb") as f:
    m = tomllib.load(f)
for entry in m.get("frames", []):
    in_path  = os.path.join(suite_dir, entry["in_file"])
    out_path = os.path.join(suite_dir, entry["out_file"])
    print(f"{entry['n']}\t{in_path}\t{out_path}")
PY
}

total_pass=0
total_fail=0
failed_frames=()

for suite in "${SUITES[@]}"; do
    suite_dir="$GOLDENS_ROOT/$suite"
    manifest="$suite_dir/manifest.toml"
    if [ ! -f "$manifest" ]; then
        echo "[$suite] manifest.toml missing — skipped" >&2
        continue
    fi
    echo "==> [$suite] running regression (manifest-driven)"
    while IFS=$'\t' read -r frame_n in_raw out_raw; do
        [ -n "$in_raw" ] || continue
        result=$("$BIN" "$in_raw" "$out_raw")
        ec=$?
        if [ $ec -eq 0 ]; then
            total_pass=$((total_pass+1))
            echo "OK   $result"
        else
            total_fail=$((total_fail+1))
            failed_frames+=("$suite/$(basename "$in_raw")")
            echo "FAIL $result"
        fi
    done < <(list_frames "$manifest" "$suite_dir")
done

echo "======================"
echo "PASS: $total_pass  FAIL: $total_fail"
if [ $total_fail -gt 0 ]; then
    echo "Failed frames:"
    printf "  %s\n" "${failed_frames[@]}"
    exit 1
fi
