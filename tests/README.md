# smooth-mod Regression & Benchmark Tests

## Ingredients

- `fixtures/*.png` — generated pixel-art test images (see `gen_test_images.py`)
- `goldens/<suite>/manifest.toml` — committed metadata + per-file SHA256 for each suite (e.g. `v1.4.0-ae2025`)
- `goldens/<suite>/*.raw` — reference SMDP dumps; **not committed** (size); fetched via `fetch_goldens.sh` once Step 4 uploads tar.zst
- `.venv/` — local Python env (Pillow only) — gitignored
- `gen_test_images.py` — regenerate fixtures
- `compare_raw.py` — pixel-diff two SMDP raw dumps (added on first use)
- `fetch_goldens.sh` — manifest-driven SHA256 verifier + (Step 4+) artifact downloader
- `run_regression.sh` — manifest-driven regression runner

## Baseline capture procedure (manual, one-off per baseline)

1. **Build bench-enabled plugin**:

   ```sh
   xcodebuild -project Mac/smooth.xcodeproj -configuration Release -arch arm64 \
       ONLY_ACTIVE_ARCH=NO \
       HEADER_SEARCH_PATHS="<SDK>/Examples/Headers <SDK>/Examples/Headers/SP <SDK>/Examples/Util <SDK>/Examples/Resources" \
       GCC_PREPROCESSOR_DEFINITIONS='SMOOTH_BENCH=1 $(inherited)' \
       MACOSX_DEPLOYMENT_TARGET=10.13 \
       CONFIGURATION_BUILD_DIR=Mac/build/bench build
   ```

2. **Install bench plugin**: copy `Mac/build/bench/smooth.plugin` into `~/Library/Application Support/Adobe/Common/Plug-ins/<ver>/MediaCore/` (or the AE2025 Plug-ins dir).

3. **Run AE from Terminal directly** (keeps stderr attached to the terminal — `open -a` detaches it and only `timing.log` would be available):

   ```sh
   rm -rf /tmp/smooth_bench
   "/Applications/Adobe After Effects 2025/Adobe After Effects 2025.app/Contents/MacOS/After Effects"
   ```

   The Mach-O binary is named `After Effects` (not `Adobe After Effects 2025`). `timing.log` is written regardless, but live stderr is handy for spotting issues.

4. **Create a test composition**:
   - Import each `tests/fixtures/*.png` as a separate footage
   - Create a 1-frame composition per fixture matching the image size
   - Apply Effect > smooth (Smooth) with default params
   - Also create variants with a couple of non-default `range` / `line weight` values to exercise parameter combinations

5. **Trigger render**: scrub the timeline or press Space. Each render call appends to `/tmp/smooth_bench/timing.log` and drops `frame_NNNN_in.raw` / `frame_NNNN_out.raw`.

6. **Save goldens**:

   ```sh
   mkdir -p tests/goldens/v1.4.0-ae2025
   cp /tmp/smooth_bench/frame_*.raw tests/goldens/v1.4.0-ae2025/
   cp /tmp/smooth_bench/timing.log  tests/goldens/v1.4.0-ae2025/
   ```

7. Record the mapping (fixture → frame index) in `goldens/v1.4.0-ae2025/index.md`.

8. Regenerate `manifest.toml` (per-frame SHA256 + SMDP header backfill). Inline Python OK; see commit `feat(phase-2a): Phase 2-A.2 Step 3` for the script body if you need the canonical version.

## Regression check (after modifications)

- Single command: `tests/run_regression.sh` (manifest-driven; iterates every suite under `tests/goldens/<suite>/manifest.toml`).
  - Set `SMOOTH_PARALLEL=0` to test the serial path; `SMOOTH_PARALLEL=1` (default) tests the rayon parallel path.
  - Verifies SHA256 of every fixture against the manifest before running, so a corrupted .raw is caught up front.
- Want to spot-check one suite: `tests/run_regression.sh v1.4.0-ae2025`.
- Want to verify integrity only (no build/run): `tests/fetch_goldens.sh [suite]`.
- Manual single-frame diff: `python3 tests/compare_raw.py tests/goldens/v1.4.0-ae2025/frame_0000_out.raw /tmp/smooth_bench/frame_0000_out.raw`.
- Timing comparison: compare `timing.log` line by line, expect only `ms=` to change.

## Raw file format (`SMDP`)

64-byte header followed by `rowbytes * height` bytes of pixels (ARGB, 8 / 16 / 32 bpc; 32bpc lands in Phase 2-A.2 Step 4).
See `bench.h` `DumpHeader` for the exact layout.

## Manifest schema (`tests/goldens/<suite>/manifest.toml`)

Documented in `docs/PHASE_2A_GPU_RFC.md` §3.2.6. Two policy slots — `mac_reference_policy` (Mac CPU bit-for-bit) vs `cross_platform_policy` (Mac↔Win tolerance) — kept separate so a near-ID exception for one doesn't accidentally relax the other. Per-frame `policy_overrides` exists for cases like frame 135 (Phase 1 strip-parallel boundary residual). Future 32bpc suite adds an `f32_abs` metric variant.

## SMDP file format

64-byte header + raw pixels (ARGB, contiguous rows; rowbytes from header).

- **v1**: 8/16bpc. `params_range` is the u32 sum threshold, `params_range_f32` slot is unused (read as 0).
- **v2**: adds 32bpc support. `params_range` is 0 on 32bpc dumps; `params_range_f32` (offset 44) carries the f32 sum threshold = `slider × 4 / 100`. v1 readers can ignore the new field; v2 readers should consult `params_range_f32` only when `bpc == 32`. See `bench.h::DumpHeader` for the canonical layout.

## 32bpc goldens capture (Phase 2-A.2 Step 4)

Capture from AE 2025 32bpc projects via EXR export → `tests/capture_32bpc.py`:

```sh
# One-time: install OpenEXR + numpy into the existing tests/.venv
tests/.venv/bin/pip install -r tests/requirements-capture.txt

# Sanity check the SMDP encoder without touching AE
tests/.venv/bin/python3 tests/capture_32bpc.py --self-test

# Per-frame capture
tests/.venv/bin/python3 tests/capture_32bpc.py \
    --frame-n 200 \
    --in-exr  /tmp/exr/input_0200.exr \
    --out-exr /tmp/exr/output_0200.exr \
    --range 12.0 --line-weight 0.5351 \
    --output-dir tests/goldens/v1.6.0-32bpc/
```

The script's docstring documents the AE Render Queue setup (two passes — one with smooth bypassed, one with it applied — exporting to EXR float). After 14 frames are captured, regenerate `tests/goldens/v1.6.0-32bpc/manifest.toml` (same backfill flow as the v1.4.0 suite) and run `tests/run_regression.sh` — the harness already dispatches `bpc == 32` to `smooth_core::process<PF_PixelFloat>`.
