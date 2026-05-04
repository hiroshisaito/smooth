# Phase 2-A.3 GPU port — line-level blending design memo

Author: design-review pass for Sub-stage C-2.5b.2-prep2b+ scope freeze
Repo state: commit 794772f, `smooth_combined` MSL kernel handling mode_flg=15 only.

---

## 1. Algorithm complexity baseline

Concrete entity counts the prep2b+ work has to cover (CPU side, post-fast-compare gate at `process.rs:82`):

**Scan helpers (count_length family)** — 9 distinct functions:
- `link8.rs:14` `count_length` (currently unused, kept for signature parity)
- `link8.rs:35` `count_length_two_lines` (used by all four `link8_mode_*` and `link8_square_blend_outside`)
- `up_mode.rs:7,64,137,211` `up_mode_{left,right,top,bottom}_count_length`
- `down_mode.rs:7,61,132,186` `down_mode_{left,right,top,bottom}_count_length`

**Blend helpers** — 8 distinct multi-pixel writers + 1 single-pixel writer:
- `blend.rs:62` `blend_line` (writes `ceil(length * line_weight)` pixels along a ray)
- `link8.rs:65` `blend_outside` and `link8.rs:98` `blend_inside` (the latter into local `temp_pixel` then averaged back, so the GPU-relevant write is only `blend_outside` plus the final averaging loop at `link8.rs:270-294`)
- `up_mode.rs:265,287,313,340` and `down_mode.rs:257,279,307,331` — eight `*_blending` helpers, each writes 1..N pixels along one cardinal ray
- `lack.rs:8,37,66` `lack_mode_{01,02,0304}_execute` — single-pixel writes at the centre, but with a *scan* phase that walks up to 3 pixels in two directions

**Maximum write distance per source pixel**:
- `link8_square_execute` writes the centre pixel + up to 4 outside-blend rays of length up to `MAX_LENGTH=128` each (`link8.rs:7`). So one source pixel at (i, j) can write to pixels as far as (i ± 128, j) or (i, j ± 128).
- `up_mode_*` / `down_mode_*` blending writes at most `(start − end) * line_weight` ≤ `core[k].length` pixels per side, bounded by image edges.
- Empirically the worst-case fan-out per source pixel that hits mode_flg=15 is **~512 writes** (4 × 128 outside + 1 centre); for mode_flg ∈ {7, 11, 13} via `link8_execute` it is ~128 writes (one outside ray each side + an inside-temp fold).

**Cross-mode shared state**:
- `BlendingInfo.core[4]: Cinfo` carries `start`, `end`, `length`, `flg` (`types.rs:60-90`). The `CR_FLG_FILL` bit is set inside the count_length helpers (`up_mode.rs:20,94,168,222`) and consumed by the dispatcher at `process.rs:119-124` and `:132-137` to choose `weight = 0.5` vs `line_weight`. **Each `core[k]` is consumed only by mode_flg=3 and =5**, so it does not cross between mode_flg cases — it is purely intra-pixel scratch, which is good news for GPU port.
- `lack_flg` is *cross-pixel*: set at `process.rs:97-99` when the current pixel has `mode_flg & 1` (rightward edge) and is consumed at `process.rs:70-80` by the *next* pixel in the same row. Pure forward dependency along a row.
- `SECOND_COUNT` flag in `up_mode_*_count_length` recursion (`up_mode.rs:31`, etc.): each count_length helper recursively re-invokes itself on a neighbour with `flag = SECOND_COUNT` to look up an "is the line one shorter, then half-pixel adjust" condition. This is a **bounded one-step recursion**, not a chain — no problem for GPU.

---

## 2. Option (a) — algorithm inversion

**Per-thread spec** (the destination pixel `(x', y')` reads anything that could write it):
- Scan a square of radius `MAX_LENGTH+2 = 130` around `(x', y')` for any pixel `(x, y)` whose mode_flg case writes at `(x', y')`.
- For each such candidate source pixel:
  - Recompute the source's full mode_flg.
  - Recompute the source's `core[]` via `up_mode_*_count_length` / `down_mode_*_count_length` (4 helpers each, each is its own up-to-128-pixel scan with one recursive sub-scan).
  - Recompute the source's blend-loop iteration count and its specific contribution at `(x', y')`.
  - Combine contributions deterministically.

**Read scope per thread**: `(2 × 130 + 1)² ≈ 68k` neighbour reads in the worst case, each of which may itself trigger up to 4 × 128 = 512-pixel scans (the source's count_length). Order **10⁷ reads per output pixel**, ~10¹³ for a 4K frame. This is a non-starter on memory bandwidth alone.

A pruned version (read radius 32, skip count_length recomputation by caching mode_flg in an intermediate) gets back to feasibility but reintroduces option (b)'s intermediate buffers — see §3.

**LOC / sessions**: Even ignoring perf, the inversion logic is *new* code with no CPU counterpart for line-by-line review. Each of the 8 line-blend helpers needs its own inversion. Realistic estimate: **8–14 sessions**, ~1500 LOC of new MSL plus matching Rust dispatch wrappers, plus a substantial new test rig because the CPU code is no longer the line-by-line oracle.

**Specific failure modes**:
- **`lack_flg` propagation across columns** (`process.rs:64-80, 97-99`): The "set in column N affecting column N+1" dependency is intrinsically serial. Inversion has to encode "column N's mode_flg & 1 was set" as a precomputed mask (*another* intermediate buffer, contradicting the prep2a no-intermediates win).
- **`SECOND_COUNT` recursive count_length** (`up_mode.rs:31-46`): each recursion examines a neighbour pixel's count_length result. To invert, thread `(x', y')` may need its source's neighbour's count_length too — a 2-hop scan that is already implicit in the CPU recursion but explicit in the inversion. Bounded but large.
- **Inside-temp averaging in `link8_execute`** (`link8.rs:270-294`): the CPU averages two `temp_pixel[0..2]` arrays computed in one thread; thread (x', y') would need to recompute *both* arrays from the source's perspective and average just the index that lands on (x', y'). Doable but adds another doubling of work per blend write.

**Determinism**: deterministic *if the combine rule is fixed* (e.g., "sum all contributions, divide by count"). However, the CPU's "later writes win" semantics (`process.rs` row-major scan, blend writes overwriting) means **bit-identical CPU↔GPU is unachievable by inversion**. The closest deterministic match is "max-priority source wins, ties broken by (y, x) lexicographic order" — that produces a *different* but consistent image. Visually equivalent? Probably yes for ~99% of pixels; for the 1% on multi-edge intersections, divergence will be visible at ≤2 ULP-magnitude differences but in different *spatial positions* than CPU.

---

## 3. Option (b) — multi-pass with `gpu_suite`-allocated intermediates

**Pass plan** (5 dispatches per frame, plus the existing preprocess+detect):

| # | Kernel | Reads | Writes | Conflicts |
|---|---|---|---|---|
| 1 | preprocess (white-key) | src | src' | none |
| 2 | detect (mode_flg byte) | src' | modes[] | none |
| 3 | mode15 centre (existing `smooth_combined`) | src', modes | dst (own pixel only) | none |
| 4 | mode15 outside-blend × 4 directions | src', modes | dst (line in one direction) | overlapping rays from neighbours |
| 5 | mode {3,5} up/down corner blend | src', modes | dst (4 cardinal rays) | same |
| 6 | mode {7,11,13} link8 outside-blend | src', modes | dst (2 rays + inside temp) | same |
| 7 | mode_flg2==3 突起 + lack_03/04 | src', modes | dst (1 pixel) | minimal |

Per the workbench note (workbench_history.md:1925-1930), these intermediates **must** go through `PF_GPUDeviceSuite1::AllocateDeviceMemory` (commit `c7e164a` regression). Adding 1–2 modes-buffers (`width × height × 1B`) is cheap (≤8 MB at 8000²); adding a full BGRA128 scratch is `width × height × 16B` (≤976 MB at 8000², ≤127 MB at 4K) — exactly the pressure that tripped commit `084b470`. So **the design rule is: scratch must be 1 byte/pixel, never 16**, which is satisfiable for `modes[]` but not for an "accumulator" buffer.

**LOC / sessions**: roughly 1 kernel per existing CPU helper, plus a Rust dispatcher wrapper. **5–7 sessions**, ~700 LOC of MSL + ~200 LOC of Rust. Less new logic than (a) because each kernel can be a near-line-by-line port of a single `*_blending` function.

**Inter-thread write conflicts** (the hard problem):
- Pass 4 (mode15 outside-blend): two source pixels at `(i, j)` and `(i+1, j)` both with mode_flg=15 produce leftward and rightward rays that *overlap* in the strip between them. CPU semantics: the second source's writes overwrite the first's. GPU semantics: undefined unless we serialise.
- Decomposition by direction (e.g., "pass 4a = leftward only, pass 4b = rightward only") still has conflicts: two source pixels in the same row both with leftward rays, separated by < 128, will both write into pixels in between. The CPU "later wins" is `i+1`'s leftward ray overwriting `i`'s leftward ray near `i`.
- **Resolution requires per-direction *priority buffer*** (1 byte/pixel: "what's the lowest source-i index that has written here"). atomicMin over a `uint` or `ushort` gives deterministic CPU-equivalent ordering. This adds 1–2 more 1-byte/pixel intermediates — still within the workbench memory budget.

**Determinism**: With atomic priority buffer, **bit-identical to CPU is achievable** because the "later wins" semantics is reproduced exactly. This is the strongest argument for (b) over (a).

---

## 4. Option (c) — partial GPU implementation

Re-evaluating against Hiroshi's 2026-05-04 hard requirement (no machine-by-machine differences in network rendering, no visible discontinuity at GPU→CPU mid-stream fallback):

**Visual divergence magnitude estimate** at a typical 4K 32bpc frame:
- mode_flg=15 fires only at isolated-pixel-against-different-neighbours configurations — empirically <0.5% of edge pixels.
- mode_flg ∈ {3, 5, 7, 11, 13} fires at **most edge pixels**: a typical 4K vector-graphics frame has ~50k–500k edge pixels, and ~80–95% of those go through these line-level blends.
- Skipping them (current GPU behaviour) leaves the input pixel unsmoothed at every edge. Visible: **yes, glaringly** — this is exactly what the smooth filter is supposed to remove.

So a network render that silently fell back from GPU to CPU mid-Render-Queue would produce frames where the first half has visible jaggies (GPU partial) and the second half is properly smoothed (CPU). **Option (c) violates the hard requirement and is rejected.**

---

## 5. Win CUDA compatibility

- **Option (a)**: translates cleanly. The inversion logic is pure compute, no Metal-specific features. Same SIMT/SIMD divergence concern (count_length scan length is data-dependent, so warp/wavefront divergence is identical on both backends). No fork risk.
- **Option (b)**: also translates. `PF_GPUDeviceSuite1::AllocateDeviceMemory` returns `CUdeviceptr` on Win and `MTLBuffer` on Mac (per `gpu_suite` API spec the workbench cites at workbench_history.md:1928-1930), so the intermediate plumbing is platform-neutral. The atomic-priority pattern uses `atomicMin` (CUDA) ↔ `atomic_min_explicit` (MSL) — both provide 32-bit unsigned atomics with relaxed ordering, semantics compatible. **No fork.**
- **SIMT vs SIMD width difference** (CUDA warp=32, Metal SIMD-group=32 on Apple Silicon, 16 on Intel Mac): only matters for the inner scan loops. Both options have the same warp-divergence pattern (some threads have count_length=2, others=128, lockstep wastes cycles). Optimisation knobs differ but algorithm is shared.
- **Fork risk**: low for both (a) and (b). The risk that *would* force a fork — different memory models — is sidestepped by routing through `PF_GPUDeviceSuite1`.

---

## 6. Recommendation

**Go with option (b): multi-pass + `gpu_suite` intermediates + atomic priority buffer for write-conflict resolution.**

Rationale: (i) it's the only option that achieves bit-identical CPU↔GPU output, which is the strongest possible answer to Hiroshi's multi-machine + fallback-continuity requirement; (ii) per-pass kernels are line-by-line ports of existing CPU helpers, so each kernel has a clear oracle for review; (iii) the memory pressure that killed commit `c7e164a` is avoided by keeping all new intermediates at 1 byte/pixel (modes byte, two 4-byte priority maps), totalling ≤270 MB at 8000² — well below the workbench 4-GB-GPU budget at MFR≤4.

**Concrete next steps (prep2b scope)**:
1. **prep2b.1**: add a per-direction `mode15_outside_priority_v` and `_h` buffer (`uint32`/pixel × 2), allocated through `gpu_suite->AllocateDeviceMemory`. Wire allocation+free into the existing dispatcher next to `modes[]`. Verify on real device that adding 32 MB at 4K does not retrigger the AE warning (this is the one experiment that gates the whole option).
2. **prep2b.2**: implement `smooth_blend_mode15_outside` MSL kernel — port `link8_square_blend_outside` (link8.rs:390-405) directly, using `count_length_two_lines` (already ported), and writing to dst with `atomic_min` on the priority buffer keyed by source linear index `y * width + x`.

**Stop-and-reconsider trigger**: if prep2b.1 (just adding two `uint32`/pixel intermediates) re-triggers the "smooth did not render anything" warning under MFR + 4K 32bpc on real device, then the gpu_suite path is *also* memory-pressure-sensitive, and the only remaining route is option (a) inversion — at which point we accept "visually equivalent, not bit-identical", document the divergence policy, and budget 8–14 sessions instead of 5–7. Specifically: if commit-N's prep2b.1 produces the AE warning on the same 4K 32bpc test that commit `8001aca` failed and `084b470` fixed, switch tracks.
