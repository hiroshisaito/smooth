#!/usr/bin/env bash
# smooth-mod-v1.5.0 パフォーマンスベンチ
# process() を各 golden フレームに対し N 回繰り返し、平均/最小時間を記録
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GOLDENS="$ROOT/tests/goldens/v1.4.0-ae2025"
BIN="$ROOT/tests/regression_test"
REPEAT="${1:-20}"

# 再ビルド(SMOOTH_PARALLEL を CLI から渡せるように)
SDK="$ROOT/references/AfterEffectsSDK_25.6_61_win/ae25.6_61.64bit.AfterEffectsSDK"
MODE_FLAG="${SMOOTH_PARALLEL:-1}"
RUST_CRATE="$ROOT/rust/smooth_core"
RUST_LIB="$RUST_CRATE/target/universal/release/libsmooth_core.a"
"$RUST_CRATE/build-universal.sh" >/dev/null || { echo "cargo build failed"; exit 1; }
clang++ -std=c++17 -O2 \
    -DSMOOTH_PARALLEL=$MODE_FLAG \
    -I"$SDK/Examples/Headers" \
    -I"$SDK/Examples/Headers/SP" \
    -I"$SDK/Examples/Util" \
    -I"$RUST_CRATE/include" \
    -I"$ROOT" \
    "$ROOT/tests/regression_test.cpp" \
    "$ROOT/util.cpp" \
    "$RUST_LIB" \
    -o "$BIN" 2>/dev/null || { echo "build failed"; exit 1; }
echo "# built with SMOOTH_PARALLEL=$MODE_FLAG"

# 代表フレームを bpc / サイズ別に計測
# (64x64 はノイズ過大なのでスキップ)
FRAMES=(
    "0135"  # 8bpc  2512x1412
    "0200"  # 8bpc  3840x2160
    "0500"  # 16bpc 3840x2160
    "1000"  # 16bpc 1920x1080
    "1500"  # 16bpc 1920x1080
    "1767"  # 16bpc 1920x1080
)

printf "%-8s %s\n" "repeat=" "$REPEAT"
for n in "${FRAMES[@]}"; do
    in_raw="$GOLDENS/frame_${n}_in.raw"
    out_raw="$GOLDENS/frame_${n}_out.raw"
    if [ -f "$in_raw" ] && [ -f "$out_raw" ]; then
        "$BIN" "$in_raw" "$out_raw" repeat "$REPEAT" | grep -E '^BENCH|^DIFF'
    fi
done
