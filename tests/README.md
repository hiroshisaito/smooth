# smooth-mod-v1.5.0 Regression & Benchmark Tests

## Ingredients

- `fixtures/*.png` — generated pixel-art test images (see `gen_test_images.py`)
- `goldens/` — reference `.raw` dumps captured from baseline build
- `.venv/` — local Python env (Pillow only) — gitignored
- `gen_test_images.py` — regenerate fixtures
- `compare_raw.py` — pixel-diff two SMDP raw dumps (added on first use)

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

## Regression check (after modifications)

- Re-run the bench build with the modified source
- Same AE comp, same params, same order → new `/tmp/smooth_bench/` dumps
- Diff against `goldens/v1.4.0-ae2025/`:

  ```sh
  python3 tests/compare_raw.py tests/goldens/v1.4.0-ae2025/frame_0000_out.raw /tmp/smooth_bench/frame_0000_out.raw
  ```

- Timing comparison: compare `timing.log` line by line, expect only `ms=` to change.

## Raw file format (`SMDP`)

64-byte header followed by `rowbytes * height` bytes of pixels (ARGB, 8 or 16 bpc).
See `bench.h` `DumpHeader` for the exact layout.
