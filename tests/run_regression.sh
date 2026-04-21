#!/usr/bin/env bash
# smooth-mod-v1.5.0 Step 3 回帰テスト runner
#
# goldens/v1.4.0-ae2025/ の frame_NNNN_in.raw に対して smooth_core::process を適用し、
# frame_NNNN_out.raw と byte-identical か確認する。
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="$ROOT/references/AfterEffectsSDK_25.6_61_win/ae25.6_61.64bit.AfterEffectsSDK"
GOLDENS="$ROOT/tests/goldens/v1.4.0-ae2025"
BIN="$ROOT/tests/regression_test"

echo "==> build regression harness"
clang++ -std=c++17 -O2 \
    -I"$SDK/Examples/Headers" \
    -I"$SDK/Examples/Headers/SP" \
    -I"$SDK/Examples/Util" \
    -I"$ROOT" \
    "$ROOT/tests/regression_test.cpp" \
    "$ROOT/util.cpp" \
    "$ROOT/upMode.cpp" \
    "$ROOT/downMode.cpp" \
    "$ROOT/Lack.cpp" \
    "$ROOT/8link.cpp" \
    -o "$BIN" || { echo "build failed"; exit 1; }

pass=0
fail=0
failed_frames=()

for in_raw in "$GOLDENS"/frame_*_in.raw; do
    [ -f "$in_raw" ] || continue
    out_raw="${in_raw/_in.raw/_out.raw}"
    [ -f "$out_raw" ] || { echo "missing: $out_raw"; continue; }

    result=$("$BIN" "$in_raw" "$out_raw")
    ec=$?
    if [ $ec -eq 0 ]; then
        pass=$((pass+1))
        echo "OK   $result"
    else
        fail=$((fail+1))
        failed_frames+=("$(basename "$in_raw")")
        echo "FAIL $result"
    fi
done

echo "======================"
echo "PASS: $pass  FAIL: $fail"
if [ $fail -gt 0 ]; then
    echo "Failed frames:"
    printf "  %s\n" "${failed_frames[@]}"
    exit 1
fi
