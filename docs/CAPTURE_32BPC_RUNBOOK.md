# Phase 2-A.2 Step 4b — 32bpc goldens capture runbook

Operational checklist for capturing the 14 reference frames in 32bpc
(`PF_PixelFloat` / ARGB128) on Mac AE 2025 and turning them into a
`tests/goldens/v1.6.0-32bpc/` suite the regression harness can replay.

This is a **once-per-suite** procedure (re-runs only when one of the
re-capture conditions in [`PHASE_2A_GPU_RFC.md` §3.2.6](PHASE_2A_GPU_RFC.md)
fires). Allow ~60 minutes the first time; subsequent runs are faster.

---

## 0. Prerequisites

- Mac AE 2025 installed at `/Applications/Adobe After Effects 2025/`.
- The smooth plugin from at least commit `0cc9a25` (Phase 2-A.2 Step 2 —
  the `FLOAT_COLOR_AWARE` flag must be set, otherwise AE will not feed
  32bpc to the effect).
- The 14-frame source material (PNG fixtures committed under
  `tests/fixtures/` or the original AE project from the v1.4.0 capture).
- Python 3.11+ in `tests/.venv/` with capture deps:
  ```sh
  tests/.venv/bin/pip install -r tests/requirements-capture.txt
  tests/.venv/bin/python3 tests/capture_32bpc.py --self-test  # must print OK
  ```
- ~3 GB free disk for the EXR scratch directory (frame 200 alone is
  3840×2160 f32 = ~127 MB per EXR, two passes per frame).

---

## 1. AE project setup

Two paths depending on whether the v1.4.0 capture project is at hand.

### Path A — reuse the v1.4.0 project (preferred)

1. Open the existing project. **File → Project Settings → Color**:
   - Depth: **32 bits per channel (float)**
   - Working space: leave at the same value used for v1.4.0 (consistency
     matters more than absolute correctness for goldens).
2. Save as a new file (e.g. `smooth_v160_32bpc_goldens.aep`) so the v1.4.0
   project on disk stays at its 8/16bpc setting.
3. Skip to step 2 below.

### Path B — recreate from scratch

1. New project, **File → Project Settings → Color → Depth: 32 bits per
   channel (float)**.
2. Import every PNG under `tests/fixtures/` as footage.
3. For each fixture, create a 1-frame composition matching the image
   dimensions (640×480 PNG → 640×480 comp at 1 frame). Drop the footage
   in as the only layer.
4. Concatenate the 14 comps into a master comp on a single timeline,
   placing each at the frame number listed in the
   [`v1.4.0 manifest`](../tests/goldens/v1.4.0-ae2025/manifest.toml)
   (frame 0, 10, 47, 50, 100, 135, 200, 500, 700, 1000, 1300, 1500,
   1700, 1767). The frame numbers are arbitrary labels — what matters
   is that AE writes one EXR per `n` value.

   Tip: if exact frame layout is awkward, use 14 separate Render Queue
   items instead of one concatenated comp; the script does not care
   about the frame index in the file name as long as the
   `capture_config_32bpc.toml` `n` field matches.

---

## 2. Per-frame parameter setup

For each frame, apply the smooth effect to the layer and set:
- `range`        → see the `range` column in
  [`tests/capture_config_32bpc.toml.template`](../tests/capture_config_32bpc.toml.template)
- `line weight`  → see `line_weight` column
- `transparent`  → see `white` column

The `range` values in the template are reverse-derived from the v1.4.0
u32 thresholds (`u32 × 100 / (max × 4)`). They are within ~2% of the
original slider values. **If you remember the original "nice" slider
value used in the v1.4.0 capture (e.g. `1.0` instead of `0.9804`),
prefer the original** — it produces a cleaner reference and a future
operator looking at the manifest will not have to reverse-engineer the
intent. Update both AE and the local
`tests/capture_config_32bpc.toml` (copy of the template) if you do.

---

## 3. Render Queue: two passes

For each frame, AE needs to emit:
- `input_NNNN.exr`  — the layer pixels as they would arrive at smooth
  **with the smooth effect bypassed**.
- `output_NNNN.exr` — the same scene **with smooth applied** at the
  configured params.

The cleanest workflow:

1. **Pass 1 — input EXRs (smooth disabled)**
   - Disable the smooth effect on every layer (the eye/`fx` toggle in the
     Effect Controls panel, *not* deletion — we want it back for pass 2).
   - **Composition → Add to Render Queue** for the master comp (or each
     of the 14 single-frame comps).
   - For each Render Queue item, set:
     - **Output Module → Format**: `OpenEXR Sequence`
     - **Format Options**:
       - Compression: `None` (avoids any reversible-but-not-bit-identical
         re-encoding on re-capture)
       - **Channels**: `RGBA` (the script needs all four; `RGB` only will
         fail at read time)
       - **Encoding**: `Float (32 bpc)` — explicitly, *not* half-float
     - **Output To**: `/tmp/exr_32bpc/input_[####].exr` (matches
       `exr_base_dir` + the template's `in_exr` filenames).
   - Click **Render**. Verify each `input_NNNN.exr` lands in
     `/tmp/exr_32bpc/`.

2. **Pass 2 — output EXRs (smooth enabled)**
   - Re-enable the smooth effect on every layer.
   - Repeat the Render Queue setup, but **Output To**:
     `/tmp/exr_32bpc/output_[####].exr` (matches the template's `out_exr`
     filenames).
   - Click **Render**.

3. Sanity-check: `/tmp/exr_32bpc/` should contain 28 files
   (`input_0000.exr` … `output_1767.exr`).

   ```sh
   ls /tmp/exr_32bpc/ | wc -l   # expect 28
   ```

---

## 4. Convert EXR pairs → SMDP

```sh
cp tests/capture_config_32bpc.toml.template tests/capture_config_32bpc.toml
# Edit tests/capture_config_32bpc.toml only if you used non-default paths
# or "nice" slider values that differ from the template.

tests/.venv/bin/python3 tests/capture_32bpc.py \
    --config tests/capture_config_32bpc.toml --verbose
```

The script emits two `.raw` files per frame to
`tests/goldens/v1.6.0-32bpc/` (28 total). The `--verbose` output reports
NaN / Inf / overbright / min / max per EXR — sanity-check before
proceeding:
- NaN/Inf > 0 in **input** EXRs is a capture rig problem (most likely
  a wrong format option in step 3); fix and re-render.
- Overbright (>1.0) in **output** is fine if the input also has it; both
  zero or both nonzero is the consistency check.

---

## 5. Generate `tests/goldens/v1.6.0-32bpc/manifest.toml`

Use the same backfill flow as the v1.4.0 suite (an inline Python script
extracts SMDP headers + SHA256). Reference recipe:

```sh
tests/.venv/bin/python3 - <<'PY'
import os, struct, hashlib
GOLDENS = "tests/goldens/v1.6.0-32bpc"
def parse(p):
    with open(p, "rb") as f: hdr = f.read(64)
    return dict(
        version=struct.unpack_from("<I", hdr, 4)[0],
        width=struct.unpack_from("<I", hdr, 8)[0],
        height=struct.unpack_from("<I", hdr, 12)[0],
        bpc=struct.unpack_from("<I", hdr, 16)[0],
        rowbytes=struct.unpack_from("<I", hdr, 20)[0],
        frame_n=struct.unpack_from("<I", hdr, 28)[0],
        range_u32=struct.unpack_from("<I", hdr, 32)[0],
        line_weight=struct.unpack_from("<f", hdr, 36)[0],
        white=struct.unpack_from("<I", hdr, 40)[0],
        range_f32=struct.unpack_from("<f", hdr, 44)[0],
    )
# … emit TOML with [suite], suite-level mac_reference_policy
# (kind="identical") and cross_platform_policy
# (kind="near-id", metric="f32_abs", max_abs=1e-5), then
# per-frame [[frames]] entries with width / height / bpc=32 /
# rowbytes / range_f32 / line_weight / white / in_file /
# out_file / in_sha256 / out_sha256 / in_size / out_size.
PY
```

The committed `v1.4.0-ae2025/manifest.toml` is the structural reference
— start by copying it, then change suite metadata + per-frame entries.
**Use `metric = "f32_abs"` and `max_abs = 1e-5` for the cross-platform
policy on this suite** (RFC §3.2.6: byte_abs is meaningless across f32
rounding boundaries, see also the `regression_test.cpp` byte-diff
caveat in the SMDP v2 path).

Sanity-check:

```sh
tests/fetch_goldens.sh v1.6.0-32bpc   # should print "OK (28 files SHA256-verified)"
tests/run_regression.sh v1.6.0-32bpc  # 14/14 IDENTICAL on Mac
```

---

## 6. Tar + GitHub Release upload

```sh
cd tests/goldens/
tar -cf - v1.6.0-32bpc/*.raw | zstd -19 -o ../../goldens-v1.6.0-32bpc.tar.zst
shasum -a 256 ../../goldens-v1.6.0-32bpc.tar.zst
```

Note the `tar.zst` SHA256 — it goes into `manifest.toml`'s
`artifact_sha256` field.

Create a pre-release on GitHub (initial tag suggestion: `v1.6.0-rc1`)
and attach the tarball as a release asset. Note the asset URL — it
goes into `manifest.toml`'s `artifact_url`.

Backfill both fields in `tests/goldens/v1.6.0-32bpc/manifest.toml` and
also in `tests/goldens/v1.4.0-ae2025/manifest.toml` (Step 4a left them
empty placeholders pending the upload). Then verify a fresh-clone
fetch path works:

```sh
mv tests/goldens/v1.6.0-32bpc tests/goldens/v1.6.0-32bpc.bak
tests/fetch_goldens.sh v1.6.0-32bpc   # should download + extract + verify
diff -r tests/goldens/v1.6.0-32bpc tests/goldens/v1.6.0-32bpc.bak  # silent
rm -rf tests/goldens/v1.6.0-32bpc.bak
```

---

## 7. Harness tolerance migration

After the suite is in place, replace the hardcoded `diff < 0.01% &&
max_abs <= 32` rule in `tests/regression_test.cpp` with a
manifest-driven policy reader (CLI args from `run_regression.sh`,
populated via `tomllib`). Frame 135's `policy_overrides` becomes the
first real consumer of the override path.

Defer this if the goldens capture took longer than expected — Step 4b
can ship in two commits ("capture + manifest" then "harness migration")
if the operator wants to validate the fixtures with the existing
hardcoded rule first.

---

## 8. Workbench history + STATUS update

End the session with `workbench_history.md` Step 4b entry (capture
date, EXR rig parameters, any deviations from this runbook, gate
results) and flip Step 4 from 🟡 to ✅ in `docs/PHASE_2A_STATUS.md`.
Then proceed to Step 5 (Mac/Win cross-platform validation).
