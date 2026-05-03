# Phase 2-A.2 Step 4b — 32bpc goldens capture runbook

The `tests/goldens/v1.6.0-32bpc/` suite is generated **synthetically**
from the existing `tests/goldens/v1.4.0-ae2025/` inputs. There is no
AE Render Queue step, no `.aep` dependency, and no GitHub Release
upload. Re-capture is one shell command:

```sh
tests/synthesize_32bpc_goldens.sh
tests/run_regression.sh   # 28/28 PASS expected
```

## Why synthetic, not AE EXR

`docs/PHASE_2A_GPU_RFC.md` §3.2.6 declares the CPU 32bpc implementation
itself as the reference for v1.6.0-32bpc — there is no independent
oracle to compare against. With that license, deriving the suite from
the v1.4.0 inputs (promoted u8/u16 → f32) eliminates two blockers we
hit during the original Step 4b plan:

1. The v1.4.0 capture `.aep` was never committed and one of its source
   layers (frame 135, 2512×1412) is unrecoverable.
2. AE projects have a global colour depth setting, so reproducing the
   mixed 8/16bpc set in a single 32bpc session is impossible — the
   v1.4.0 set itself came from multiple AE sessions.

The regression role of the suite is "does `smooth_core::process<
PF_PixelFloat>` stay deterministic across builds and platforms?", not
"does it match an independent oracle?". Algorithmic correctness was
already established by Phase 1 (8/16bpc vs AE-driven goldens) plus the
overbright/NaN/subnormal unit tests in Phase 2-A.2 Step 1.

## How synthesize_32bpc_goldens.sh works

1. Verify v1.4.0-ae2025 fixtures (per-file SHA256 against manifest).
2. Build a one-shot tool `tests/synth_32bpc` linked against
   `libsmooth_core.a` with **`SMOOTH_PARALLEL=0`** baked in. Serial
   capture is mandatory for determinism — rayon strip ordering is
   reproducible only at the start/end byte boundaries, not the middle
   of large frames.
3. For each frame in the v1.4.0 manifest:
   - Read the source `.raw` (8 or 16 bpc).
   - Promote pixels to f32 (`u8 / 255` or `u16 / 32768`).
   - Promote the range threshold the same way (`u32_range / max`),
     so the normalised "same-colour" tolerance is preserved.
   - Run `smooth_core::process<PF_PixelFloat>` and write the input
     and output `.raw` files in SMDP v2 format with `bpc = 32`.
4. Regenerate `tests/goldens/v1.6.0-32bpc/manifest.toml` from the new
   files (per-file SHA256, schema unchanged from v1.4.0).
5. Re-run `fetch_goldens.sh` for SHA256 self-consistency.

## Regression behaviour

- `SMOOTH_PARALLEL=0` regression: 14/14 IDENTICAL on the v1.6.0-32bpc
  suite (re-running the exact code path that generated the goldens).
- `SMOOTH_PARALLEL=1` regression: 13/14 IDENTICAL plus frame 135
  NEAR-ID (`max_f32_abs ≈ 9.2e-2`, 30/14187776 floats differ). The
  same Phase 1 strip-parallel boundary-decision residual that frame
  135 carries in 8bpc, scaled into the f32 domain (≈ `8bpc max_abs=23
  / 255`).

`tests/regression_test.cpp` switches NEAR-ID metrics by `bpc`:
- 8/16bpc: `byte_diff_pct < 0.01% && max_byte_abs ≤ 32` (Phase 1 rule).
- 32bpc: `f32_diff_pct < 0.01% && max_f32_abs ≤ 0.125` (matches the
  same allowance translated to f32; tighter cross-platform numbers
  per RFC §3.2.6 are applied at Mac↔Win compare time, not here).

## What to do if the v1.6.0-32bpc manifest disagrees

The manifest is intentionally regeneratable. If `synthesize_32bpc_goldens.sh`
produces a manifest that differs from the committed one:
- **Expected** after a deliberate algorithm change in `smooth_core` —
  commit the new manifest as part of the same PR that changes the
  algorithm.
- **Unexpected** otherwise — investigate. The likely culprits are
  build-time non-determinism (don't edit `tests/synth_32bpc.cpp` to
  use `SMOOTH_PARALLEL=1`), a Rust toolchain change that altered f32
  reduction order, or an actual bug.

## Alternative path: AE-driven EXR (kept for HDR test material)

`tests/capture_32bpc.py`, `tests/capture_config_32bpc.toml.template`,
and `tests/requirements-capture.txt` are still in the tree. They were
written for a Render-Queue-driven capture flow and remain useful if a
future task needs HDR / overbright source material that the synthetic
path (which only sees 0..1 inputs from u8/u16 promotion) cannot
produce. They are NOT part of the Step 4b critical path; do not run
them unless you are explicitly capturing HDR fixtures.
