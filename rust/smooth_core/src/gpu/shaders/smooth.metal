// MSL kernels for the smooth GPU path.
//
// AE delivers GPU effect worlds in PF_PixelFormat_GPU_BGRA128 (per
// AE_EffectPixelFormat.h L41), so float4 components map to:
//   x = blue, y = green, z = red, w = alpha
// This is the OPPOSITE channel order from PF_PixelFloat on the CPU side
// (which is ARGB: x=alpha, y=red, z=green, w=blue). For per-channel
// access (white-key check, RGB-only compares) the MSL code reads .x/.y/.z
// = b/g/r explicitly. For sum-style comparisons (delta_sum across all 4
// channels) the order is irrelevant — do NOT import named CPU helpers
// without first deciding which class of comparison applies.

#include <metal_stdlib>
using namespace metal;

// ========================================================================
// Pixel comparison primitives (port of compare.rs).
//
// `pixel_delta_sum` mirrors `SmoothPixel::delta_sum` for Pixel32:
//   |Δa| + |Δr| + |Δg| + |Δb|
// across all four channels. The channel order does not matter for this
// sum — BGRA and ARGB give the same result.
//
// `compare_pixel(a, b, range)` returns 1 iff the pixels are "different"
// (Δ > range), matching the CPU sense — bit set in process_row_range's
// mode_flg means "this neighbour is on the OTHER side of an edge from
// the centre".
//
// `compare_pixel_equal` is the complement (Δ <= range).
//
// `fast_compare_pixel` mirrors the CPU fast path: a strict-equality
// byte compare, faster than computing delta_sum when the cheap rejection
// path is enough to gate the expensive mode_flg work.
// ========================================================================

inline float pixel_delta_sum(float4 a, float4 b) {
    float4 d = fabs(a - b);
    return d.x + d.y + d.z + d.w;
}

inline bool compare_pixel(float4 a, float4 b, float range) {
    return pixel_delta_sum(a, b) > range;
}

inline bool compare_pixel_equal(float4 a, float4 b, float range) {
    return pixel_delta_sum(a, b) <= range;
}

inline bool fast_compare_pixel(float4 a, float4 b) {
    // any() on the != returns true when at least one channel differs.
    // Matches the CPU u32/u64 "are bytes equal" gate (ignores the
    // tolerance — used as a cheap reject before compare_pixel runs).
    return any(a != b);
}

// ========================================================================
// blending_pixel_f (port of blend.rs::blending_pixel_f for Pixel32/f32).
//
// CPU formula at max_value = 1.0:
//   out.alpha = target.alpha * ratio + ref.alpha * (1 - ratio)
//   out.rgb   = target.rgb   * ratio + ref.rgb   * (1 - ratio)
// with two special cases for premultiplied-alpha edge values:
//   - target.alpha == 0  →  copy ref.rgb (unblended)
//   - ref.alpha    == 0  →  copy target.rgb (unblended)
// The "both alpha == max" branch on the CPU is just a fast-path for the
// general formula at max_value=1.0, so f32 collapses to two branches:
// one of {target,ref}.alpha is zero, or neither is.
//
// Channel order is BGRA (target.x = blue, etc), but the lerp formula is
// channel-symmetric so the result is layout-correct regardless of which
// component is which.
// ========================================================================
inline float4 blending_pixel_f(float4 target, float4 ref, float ratio) {
    const float r_alpha = 1.0f - ratio;
    const float out_a = target.w * ratio + ref.w * r_alpha;
    if (target.w == 0.0f) {
        return float4(ref.x, ref.y, ref.z, out_a);
    }
    if (ref.w == 0.0f) {
        return float4(target.x, target.y, target.z, out_a);
    }
    return float4(
        target.x * ratio + ref.x * r_alpha,
        target.y * ratio + ref.y * r_alpha,
        target.z * ratio + ref.z * r_alpha,
        out_a);
}

// C-1 plumbing kernel: identity copy src → dst. Kept for unit tests; the
// production GPU path uses smooth_preprocess (below) instead.
kernel void smooth_passthrough(
    device const float4* src        [[buffer(0)]],
    device float4*       dst        [[buffer(1)]],
    constant uint&       src_pitch  [[buffer(2)]],
    constant uint&       dst_pitch  [[buffer(3)]],
    constant uint&       width      [[buffer(4)]],
    constant uint&       height     [[buffer(5)]],
    uint2                gid        [[thread_position_in_grid]])
{
    if (gid.x >= width || gid.y >= height) return;
    dst[gid.y * dst_pitch + gid.x] = src[gid.y * src_pitch + gid.x];
}

// C-2.5b.1: preprocess. Mirrors `pre_process` in `preprocess.rs` for the
// in-place white-key stripping half (the bbox computation half is dropped
// — AE already gives us the full extent, and the row-range loop in the
// blend kernels iterates the whole picture). Pixels whose RGB equals the
// white key (1.0, 1.0, 1.0; alpha ignored) are replaced with the null
// pixel (all zeros). Other pixels are forwarded unchanged.
//
// `white_opt` is 0 / 1. With 0 this kernel degenerates to a copy.
//
// Channel layout is BGRA, so the white-key check reads p.z (red), p.y
// (green), p.x (blue) — equivalent to the CPU `rgb_eq` against
// (red=1.0, green=1.0, blue=1.0).
kernel void smooth_preprocess(
    device const float4* src        [[buffer(0)]],
    device float4*       dst        [[buffer(1)]],
    constant uint&       src_pitch  [[buffer(2)]],
    constant uint&       dst_pitch  [[buffer(3)]],
    constant uint&       width      [[buffer(4)]],
    constant uint&       height     [[buffer(5)]],
    constant uint&       white_opt  [[buffer(6)]],
    uint2                gid        [[thread_position_in_grid]])
{
    if (gid.x >= width || gid.y >= height) return;
    float4 p = src[gid.y * src_pitch + gid.x];
    if (white_opt != 0u) {
        // BGRA: .z=red, .y=green, .x=blue. CPU rgb_eq is exact equality.
        if (p.z == 1.0f && p.y == 1.0f && p.x == 1.0f) {
            p = float4(0.0f, 0.0f, 0.0f, 0.0f);
        }
    }
    dst[gid.y * dst_pitch + gid.x] = p;
}

// ========================================================================
// C-2.5b.2-prep1: smooth_detect kernel.
//
// Per pixel, write a mode-flag byte to the `modes` buffer mirroring the
// branch decision in `process_row_range`:
//
//     if (fast_compare_pixel(centre, right)) {
//         if (compare_pixel(centre, right))  modes |= 1 << 0;
//         if (compare_pixel(centre, up))     modes |= 1 << 1;
//         if (compare_pixel(centre, down))   modes |= 1 << 2;
//         if (compare_pixel(centre, left))   modes |= 1 << 3;
//         modes |= 0x80;  // mark "fast_match passed" — bits 0..3 valid
//     }
//
// Pixels whose right neighbour passes fast_compare get the meaningful
// mode_flg; everyone else gets 0. The blend kernel (C-2.5b.2-prep2 onward)
// will read this buffer and route each pixel into the appropriate case
// (mode_flg == 3, 5, 7, 11, 13, 15 each map to a CPU helper today).
//
// Edge handling: pixels outside the [1, logical_width-2] x [1, height-2]
// inner region cannot read all four cardinal neighbours; we conservatively
// emit modes = 0 for those (no smoothing, matches the CPU's eh_top/left/
// right/bottom 1-px inset).
//
// `modes` is a tightly packed `uchar` buffer with `width * height` bytes
// (no row pitch — write index = y * width + x).
kernel void smooth_detect(
    device const float4* src           [[buffer(0)]],  // BGRA128 input
    device uchar*        modes         [[buffer(1)]],  // out: 1 byte per pixel
    constant uint&       src_pitch     [[buffer(2)]],  // pixels
    constant uint&       width         [[buffer(3)]],  // physical width (= modes pitch)
    constant uint&       height        [[buffer(4)]],
    constant uint&       logical_width [[buffer(5)]],
    constant float&      range         [[buffer(6)]],
    uint2                gid           [[thread_position_in_grid]])
{
    if (gid.x >= width || gid.y >= height) return;
    const uint x = gid.x;
    const uint y = gid.y;
    const uint out_idx = y * width + x;

    // Conservative inner-region gate: same 1-px inset the CPU `process()`
    // applies to eh_top/left/right/bottom. Pixels outside this region
    // would have to read out-of-bounds neighbours; we keep the kernel
    // branchless w.r.t. neighbour bounds by skipping them entirely.
    if (x < 1u || x + 1u >= logical_width || y < 1u || y + 1u >= height) {
        modes[out_idx] = 0;
        return;
    }

    const float4 c     = src[y * src_pitch + x];
    const float4 right = src[y * src_pitch + (x + 1u)];

    if (!fast_compare_pixel(c, right)) {
        modes[out_idx] = 0;
        return;
    }

    const float4 up    = src[(y - 1u) * src_pitch + x];
    const float4 down  = src[(y + 1u) * src_pitch + x];
    const float4 left  = src[y * src_pitch + (x - 1u)];

    uint mode_flg = 0u;
    if (compare_pixel(c, right, range)) mode_flg |= (1u << 0);
    if (compare_pixel(c, up,    range)) mode_flg |= (1u << 1);
    if (compare_pixel(c, down,  range)) mode_flg |= (1u << 2);
    if (compare_pixel(c, left,  range)) mode_flg |= (1u << 3);

    // bit 7 marks the centre as "interesting" — fast_compare passed.
    // mode_flg == 0 (all neighbours within tolerance) still writes the
    // sentinel so the blend pass can distinguish "no edges" from
    // "fast_compare did not even fire" when triaging output.
    modes[out_idx] = (uchar)(mode_flg | 0x80u);
}

// White-key strip helper: read a BGRA pixel and replace it with the null
// pixel if RGB == (1, 1, 1) and white_opt != 0. Channel layout: BGRA, so
// .z=red, .y=green, .x=blue (matches the smooth_preprocess kernel).
inline float4 load_strip(float4 p, uint white_opt) {
    if (white_opt != 0u && p.z == 1.0f && p.y == 1.0f && p.x == 1.0f) {
        return float4(0.0f, 0.0f, 0.0f, 0.0f);
    }
    return p;
}

// ========================================================================
// C-2.5b.2-prep2b foundation: count_length_two_lines port.
//
// Mirrors `count_length_two_lines` in link8.rs: walk two parallel
// 1-pixel-wide rays starting at (target0_xy, target1_xy) in direction
// (`step_x, step_y`) up to MAX_LENGTH or until either ray's
// compare_pixel against its next neighbour fires. Returns the number of
// steps taken (always positive) plus a flag indicating whether ray 0
// was the one that broke. Caller's `min` / `max` / `limit_from_here`
// gate the scan to stay within the layer extent (matches the CPU
// helper's bounding semantics).
//
// Each GPU thread runs its OWN scan when its mode_flg activates a
// link8 / up_mode / down_mode case. The scan is bounded by MAX_LENGTH
// so divergence between threads has a predictable upper limit.
//
// All neighbour reads go through load_strip so the white_option is
// applied consistently with the centre kernel — same semantics as
// the CPU `compare_pixel` running on a post-preprocess buffer.
//
// Used by: smooth_combined for mode_flg ∈ {3, 5, 7, 11, 13, 15}
// (currently only mode_flg=15 wired in; line-level cases land in
// follow-up commits as the inversion / multi-pass design is finalised).

constant int SMOOTH_MAX_LENGTH = 128;  // matches link8.rs MAX_LENGTH

struct CountLenResult {
    int  length;
    bool t0_flg;  // true iff ray 0 broke first
};

inline CountLenResult count_length_two_lines(
    device const float4* src,
    uint src_pitch,
    int  target0_x, int target0_y,
    int  target1_x, int target1_y,
    int  step_x,    int step_y,
    int  min_bound, int max_bound, int limit_from_here,
    float range,
    uint white_opt,
    int  max_length)
{
    CountLenResult result;
    result.length = 0;
    result.t0_flg = false;

    // sign(step_*): step magnitudes are always 1 in the CPU helper, the
    // sign tells us which way length increments. We mirror that by using
    // step_x | step_y (only one is non-zero per CPU call site) to derive
    // the sign — a horizontal step has step_y == 0, a vertical step has
    // step_x == 0.
    const int axis_step = (step_x != 0) ? step_x : step_y;
    const int len_diff  = (axis_step > 0) ?  1 : -1;

    int length = 0;

    // Bounded loop: `max_length` iterations cap divergence between
    // threads. Step1 (prep2c-step1, 2026-05-05) replaces the previous
    // SMOOTH_MAX_LENGTH=128 with a runtime `gpu_max_length` derived from
    // env var SMOOTH_GPU_MAX_LENGTH (default 32). Lines that would have
    // extended past the cap don't reach the output pixel anyway because
    // the per-output writer scan also bounds search radius by the same
    // cap. CPU equivalence is broken at this point — that's accepted for
    // the GPU profile (see design memo §10).
    for (int iter = 0; iter < max_length; iter++) {
        const int probe = length + limit_from_here;
        if (!(min_bound < probe && probe < max_bound)) break;

        const int abs_len = (length >= 0) ? length : -length;
        const int t0x = target0_x + abs_len * step_x;
        const int t0y = target0_y + abs_len * step_y;
        const int t1x = target1_x + abs_len * step_x;
        const int t1y = target1_y + abs_len * step_y;

        length += len_diff;

        // compare_pixel(t0, t0 + step) — does ray 0 hit a colour change?
        const float4 t0_a = load_strip(src[(uint)t0y * src_pitch + (uint)t0x], white_opt);
        const float4 t0_b = load_strip(src[(uint)(t0y + step_y) * src_pitch + (uint)(t0x + step_x)], white_opt);
        if (compare_pixel(t0_a, t0_b, range)) {
            result.t0_flg = true;
            break;
        }

        // compare_pixel(t1, t1 + step) — does ray 1?
        const float4 t1_a = load_strip(src[(uint)t1y * src_pitch + (uint)t1x], white_opt);
        const float4 t1_b = load_strip(src[(uint)(t1y + step_y) * src_pitch + (uint)(t1x + step_x)], white_opt);
        if (compare_pixel(t1_a, t1_b, range)) {
            break;
        }
    }

    result.length = (length >= 0) ? length : -length;
    return result;
}

// ========================================================================
// C-2.5b.2-prep2a follow-up: smooth_combined kernel.
//
// Replaces the three-pass preprocess/detect/blend chain with ONE kernel.
// Each thread loads its centre + neighbours from `src`, applies the
// white-key strip inline at every read, computes mode_flg locally, and
// writes the smoothed pixel to its own (gid.x, gid.y) in `dst`. No
// intermediate buffers, no inter-thread dependencies, no memory-pressure
// allocation per call.
//
// Why this replaces the chain: real-device test on build 8001aca still
// occasionally tripped AE's "smooth did not render anything" warning
// even with cb.wait_until_completed() — the multi-pass design allocates
// width×height×16-byte intermediates per dispatch, and under MFR + 4K
// 32bpc that pressure (≈ 132MB × N threads) intermittently makes Metal
// or AE's GPU world tracking unhappy. Inlining everything sidesteps the
// problem at the cost of redundant reads (centre is loaded up to 5x for
// mode_flg=15 case; neighbours up to 1x each). For the BGRA128 cache,
// the redundancy is well-served by L1/L2; benchmarks pending.
//
// Currently handles only mode_flg=15 (link8_square centre averaging).
// Other mode_flg values (3, 5, 7, 11, 13) fall through to identity copy
// from src; line-level blends arrive in subsequent prep steps.
kernel void smooth_combined(
    device const float4* src           [[buffer(0)]],
    device float4*       dst           [[buffer(1)]],
    constant uint&       src_pitch     [[buffer(2)]],
    constant uint&       dst_pitch     [[buffer(3)]],
    constant uint&       width         [[buffer(4)]],
    constant uint&       height        [[buffer(5)]],
    constant uint&       logical_width [[buffer(6)]],
    constant float&      range         [[buffer(7)]],
    constant uint&       white_opt     [[buffer(8)]],
    uint2                gid           [[thread_position_in_grid]])
{
    if (gid.x >= width || gid.y >= height) return;
    const uint x = gid.x;
    const uint y = gid.y;

    // Centre with white strip applied.
    const float4 c = load_strip(src[y * src_pitch + x], white_opt);
    float4 out = c;

    // Inner region only — same 1-px inset as the standalone detect kernel.
    if (x >= 1u && x + 1u < logical_width && y >= 1u && y + 1u < height) {
        // Right neighbour (post-strip) for the fast_compare gate.
        const float4 right = load_strip(src[y * src_pitch + (x + 1u)], white_opt);
        if (fast_compare_pixel(c, right)) {
            // Build mode_flg from the four cardinal compares (post-strip).
            const float4 up   = load_strip(src[(y - 1u) * src_pitch + x], white_opt);
            const float4 down = load_strip(src[(y + 1u) * src_pitch + x], white_opt);
            const float4 left = load_strip(src[y * src_pitch + (x - 1u)], white_opt);

            uint mode_flg = 0u;
            if (compare_pixel(c, right, range)) mode_flg |= (1u << 0);
            if (compare_pixel(c, up,    range)) mode_flg |= (1u << 1);
            if (compare_pixel(c, down,  range)) mode_flg |= (1u << 2);
            if (compare_pixel(c, left,  range)) mode_flg |= (1u << 3);

            if (mode_flg == 15u) {
                // link8_square_execute centre: average four corner blends.
                const float4 d_ul = load_strip(src[(y - 1u) * src_pitch + (x - 1u)], white_opt);
                const float4 d_ur = load_strip(src[(y - 1u) * src_pitch + (x + 1u)], white_opt);
                const float4 d_br = load_strip(src[(y + 1u) * src_pitch + (x + 1u)], white_opt);
                const float4 d_bl = load_strip(src[(y + 1u) * src_pitch + (x - 1u)], white_opt);

                const float4 t_ul = compare_pixel_equal(c, d_ul, range) ? c : blending_pixel_f(c, d_ul, 0.5f);
                const float4 t_ur = compare_pixel_equal(c, d_ur, range) ? c : blending_pixel_f(c, d_ur, 0.5f);
                const float4 t_br = compare_pixel_equal(c, d_br, range) ? c : blending_pixel_f(c, d_br, 0.5f);
                const float4 t_bl = compare_pixel_equal(c, d_bl, range) ? c : blending_pixel_f(c, d_bl, 0.5f);

                out = (t_ul + t_ur + t_br + t_bl) * 0.25f;
            }
            // mode_flg ∈ {3, 5, 7, 11, 13}: identity (= centre post-strip).
        }
        // fast_compare passes bypass the algorithm entirely (CPU side too).
    }

    dst[y * dst_pitch + x] = out;
}

// ========================================================================
// C-2.5b.2-prep2c (Path β v2): smooth_blend_mode15_outside_per_output.
//
// Port of `link8_square_blend_outside` (link8.rs:390-405) using
// **per-output-pixel writer selection** to reproduce CPU row-major
// last-writer-wins semantics WITHOUT atomics or intermediate buffers.
//
// Background (2026-05-04): prior option (b) attempts (prep2b.2b
// monolithic / tile-dispatch / CreateGPUWorld variants) all FAILed at
// real-device UAT with AE warning + FrameTask 517. External review
// concluded that smooth's data-dependent atomic chain + intermediate
// buffer + async completion combination is outside AE/Metal practical
// envelope, and recommended pivoting to per-output writer selection.
// See docs/PHASE_2A_PREP2B_DESIGN_MEMO.md §7 + workbench_history.md.
//
// Algorithm: each thread = 1 output pixel (px, py). The thread scans
// 4 cardinal rays for candidate centres that might write to (px, py)
// via link8_square_blend_outside, in CPU row-major priority order, and
// computes the winning centre's blend value at our position.
//
// CPU writer ordering analysis:
//   Within link8_square_execute at centre (cx, cy), the 4 outside calls
//   write disjoint pixel sets (each call walks a single cardinal ray
//   from the centre). So per-centre, each output pixel is written by
//   AT MOST 1 outside block.
//
//   Across centres in CPU scan order (j ascending, then i ascending),
//   later centres overwrite earlier centres' writes at overlapping
//   pixels. So the "last writer" for output pixel (px, py) is the
//   centre with the largest scan-order index `cy * width + cx` among
//   all centres whose outside call lines reach (px, py).
//
// Block priority (largest to smallest cy * width + cx for candidates
// that could write to (px, py)):
//   Block 2: centres at (px,        py + 1 + k), cy = py + 1 + k → LARGEST cy
//   Block 1: centres at (px + 1 + k, py        ), cy = py, cx > px
//   Block 3: centres at (px - 1 - k, py        ), cy = py, cx < px
//   Block 4: centres at (px,        py - 1 - k), cy = py - 1 - k → SMALLEST cy
//
//   Thus we scan blocks in priority order 2 > 1 > 3 > 4. Within each
//   block we scan k = MAX_LENGTH-1 downward to 0; first valid match is
//   the largest-cy/cx winner within that block. If any block produces
//   a winner, no need to scan lower-priority blocks.
//
// Per-output cost (worst case):
//   4 blocks × 128 candidates × (compute_centre_flg ~30 ops +
//   count_length_two_lines ~1024 ops) ≈ 540K ops. With early break on
//   first match per block, average case is much faster. For 4400² this
//   is bounded GPU compute that fits comfortably under watchdog.

// Compute the 4-corner equality flg for the centre at (cx, cy), or
// 0xFF if (cx, cy) is not a mode_flg=15 centre (early-out sentinel).
// Mirrors link8_square_execute L416-420.
inline uint compute_centre_flg_15(
    device const float4* src,
    uint src_pitch,
    uint cx, uint cy,
    uint logical_width, uint height,
    float range,
    uint white_opt)
{
    if (cx < 1u || cx + 1u >= logical_width || cy < 1u || cy + 1u >= height) {
        return 0xFFu;
    }
    const float4 c     = load_strip(src[cy * src_pitch + cx], white_opt);
    const float4 right = load_strip(src[cy * src_pitch + (cx + 1u)], white_opt);
    if (!fast_compare_pixel(c, right)) return 0xFFu;

    const float4 up   = load_strip(src[(cy - 1u) * src_pitch + cx], white_opt);
    const float4 down = load_strip(src[(cy + 1u) * src_pitch + cx], white_opt);
    const float4 left = load_strip(src[cy * src_pitch + (cx - 1u)], white_opt);

    uint mode_flg = 0u;
    if (compare_pixel(c, right, range)) mode_flg |= 1u;
    if (compare_pixel(c, up,    range)) mode_flg |= 2u;
    if (compare_pixel(c, down,  range)) mode_flg |= 4u;
    if (compare_pixel(c, left,  range)) mode_flg |= 8u;
    if (mode_flg != 15u) return 0xFFu;

    const float4 ul = load_strip(src[(cy - 1u) * src_pitch + (cx - 1u)], white_opt);
    const float4 ur = load_strip(src[(cy - 1u) * src_pitch + (cx + 1u)], white_opt);
    const float4 br = load_strip(src[(cy + 1u) * src_pitch + (cx + 1u)], white_opt);
    const float4 bl = load_strip(src[(cy + 1u) * src_pitch + (cx - 1u)], white_opt);

    uint flg = 0u;
    if (compare_pixel_equal(c, ul, range)) flg |= 1u;
    if (compare_pixel_equal(c, ur, range)) flg |= 2u;
    if (compare_pixel_equal(c, br, range)) flg |= 4u;
    if (compare_pixel_equal(c, bl, range)) flg |= 8u;
    return flg;
}

// State recorded for the winning centre — enough to recompute the
// per-pixel blend value without a separate intermediate buffer.
struct WriterCandidate {
    bool  found;
    int   ref_off_x;     // ref_offset.x relative to my pixel
    int   ref_off_y;     // ref_offset.y relative to my pixel
    int   t;             // CPU iter index (0 = farthest from centre, last = adjacent)
    int   last;          // len_pixels - 1
    float len;           // effective line length (count * lw)
};

// Try to update `best` with candidate centre (cx, cy) for `block_id`.
// Validates: inner region, mode_flg=15, block fires, line reaches our
// pixel. If valid AND beats current best, returns updated state with
// found=true. The caller is responsible for ensuring this call is made
// only when (cx, cy) corresponds to the per-block candidate at offset
// k from our pixel along the appropriate ray.
//
// `k` is the distance from our pixel to the centre along the block's
// cardinal step (k=0: centre adjacent to us, k=MAX-1: centre far away).
// `t` returned for the winner = (len_pixels - 1) - k (CPU iter index
// at which our pixel is written by this block's blend_line).
//
// Returns updated WriterCandidate. If validation fails, returns the
// passed-in `best` unchanged.
// Step1 helper (prep2c-step1, 2026-05-05): compute corner equality flg
// for a centre that has ALREADY been verified as mode_flg=15 via
// metadata. Skips the mode15 check (4 cardinal compares + fast_compare)
// that compute_centre_flg_15 does, since metadata already provides that
// answer. Returns the 4-bit corner flg (UL=1, UR=2, BR=4, BL=8).
//
// Caller MUST have verified `(metadata[cy*width+cx] & 0x0Fu) == 0x0Fu`
// before calling this function. Boundary checks are also caller's
// responsibility (the metadata kernel writes 0 for boundary pixels, so
// metadata_is_mode15 = false there, which prevents reaching this).
inline uint compute_centre_corner_flg_only(
    device const float4* src,
    uint src_pitch,
    uint cx, uint cy,
    float range,
    uint white_opt)
{
    const float4 c  = load_strip(src[cy * src_pitch + cx], white_opt);
    const float4 ul = load_strip(src[(cy - 1u) * src_pitch + (cx - 1u)], white_opt);
    const float4 ur = load_strip(src[(cy - 1u) * src_pitch + (cx + 1u)], white_opt);
    const float4 br = load_strip(src[(cy + 1u) * src_pitch + (cx + 1u)], white_opt);
    const float4 bl = load_strip(src[(cy + 1u) * src_pitch + (cx - 1u)], white_opt);

    uint flg = 0u;
    if (compare_pixel_equal(c, ul, range)) flg |= 1u;
    if (compare_pixel_equal(c, ur, range)) flg |= 2u;
    if (compare_pixel_equal(c, br, range)) flg |= 4u;
    if (compare_pixel_equal(c, bl, range)) flg |= 8u;
    return flg;
}

// Step1 helper: 1 metadata byte → "is this a mode_flg=15 centre?"
// Bits 0-3 of the metadata byte are the 4-cardinal compare result.
// The detect kernel writes 0 for boundary / fast_compare-failing
// pixels so this returns false in those cases.
inline bool metadata_is_mode15(uchar m) {
    return (m & 0x0Fu) == 0x0Fu;
}

inline WriterCandidate try_block_candidate(
    WriterCandidate best,
    device const float4* src,
    device const uchar*  metadata,
    uint src_pitch,
    int  cx, int cy,
    int  k,
    int  block_id,
    int  ref_off_x_for_flg1, int ref_off_y_for_flg1,  // sub-variant 1 ref
    uint flg1_mask,
    int  ref_off_x_for_flg2, int ref_off_y_for_flg2,  // sub-variant 2 ref
    uint flg2_mask,
    uint block_fire_mask,                              // (flg & mask) == mask blocks block from firing
    int  target_x, int target_y,                      // start pixel of the line
    int  step_x, int step_y,
    int  scan_min, int scan_max, int scan_limit,
    uint width, uint height, uint logical_width,
    float range, uint white_opt, float line_weight,
    int  max_length)
{
    // Inner region check (also covered by metadata = 0 for boundary,
    // but kept defensively for the candidate (cx, cy) coords).
    if (cx < 1 || cy < 1 || uint(cx) + 1u >= logical_width || uint(cy) + 1u >= height) return best;

    // Step1: mode_flg=15 check via metadata (replaces 5-src-read +
    // 4-compare path inside compute_centre_flg_15).
    if (!metadata_is_mode15(metadata[uint(cy) * width + uint(cx)])) return best;

    // Corner equality flg still requires 4 diagonal src reads + compares.
    const uint flg = compute_centre_corner_flg_only(
        src, src_pitch, uint(cx), uint(cy), range, white_opt);

    // Block fires: (flg & block_fire_mask) != block_fire_mask
    if ((flg & block_fire_mask) == block_fire_mask) return best;

    // Determine sub-variant ref_offset
    int ref_off_x, ref_off_y;
    if ((flg & flg1_mask) != 0u) {
        ref_off_x = ref_off_x_for_flg1;
        ref_off_y = ref_off_y_for_flg1;
    } else if ((flg & flg2_mask) != 0u) {
        ref_off_x = ref_off_x_for_flg2;
        ref_off_y = ref_off_y_for_flg2;
    } else {
        return best;  // neither sub-variant fires
    }

    // count_length_two_lines from centre's perspective, bounded by cap.
    const CountLenResult clr = count_length_two_lines(
        src, src_pitch,
        target_x, target_y,
        target_x + ref_off_x, target_y + ref_off_y,
        step_x, step_y,
        scan_min, scan_max, scan_limit,
        range, white_opt, max_length);
    if (clr.length <= 0) return best;

    const float lw = clr.t0_flg ? 0.5f : line_weight;
    const float len = float(clr.length) * lw;
    const int   len_pixels = int(ceil(len));
    if (len_pixels <= 0) return best;
    const int   last = len_pixels - 1;

    // Check if line reaches my pixel: line writes pixels at offsets
    // 0..last along step from `target`. My distance from `target` is
    // exactly k (caller ensures this by construction). So line reaches
    // me iff k <= last, i.e., k < len_pixels.
    if (k >= len_pixels) return best;

    // CPU iter t at which my pixel is written: blend_line walks t = 0..last
    // writing at offset (last - t) along step. For me at offset = k,
    // t = last - k.
    const int t = last - k;

    // Within block scan, we always scan k from large to small and break
    // on first match — caller's responsibility. So `best.found = true`
    // means we already locked in the within-block winner. This function
    // is only called with best.found == false at the time of update.
    WriterCandidate updated;
    updated.found = true;
    updated.ref_off_x = ref_off_x;
    updated.ref_off_y = ref_off_y;
    updated.t = t;
    updated.last = last;
    updated.len = len;
    return updated;
}

// smooth_per_pixel: SOLE production kernel for the GPU smooth path
// (Sub-stage C-2.5b.2-prep2c-step1, 2026-05-05).
//
// Each thread = one output pixel. The thread:
// 0. Reads metadata (1 byte/pixel) for self + 4 cardinal directions up
//    to `gpu_max_length` distance. If NO mode_flg=15 candidate is found
//    in any direction (and self is not mode_flg=15), no centre within
//    cap range can write to me via line blend AND I'm not a self-inside
//    centre → safe to copy src + return. Most flat regions exit here in
//    O(1 + 4*cap) metadata reads.
// 1. Searches Block 2 then Block 1 for outside writers LATER than self-inside
//    in CPU scan order (cy > py, or cy=py with cx>px).
// 2. If no later outside found, checks via metadata if THIS pixel is a
//    mode_flg=15 centre (would do its own inside 4-corner avg write).
// 3. If neither, searches Block 3 then Block 4 for EARLIER outside writers
//    (since they're the only writers when no later writer exists).
// 4. Writes dst exactly once based on the chosen writer (or src passthrough).
//
// History: prep2c v2 (commit 2c85871) had this kernel without metadata
// or cap — it ran 4 cardinal scans of MAX_LENGTH=128 per output pixel,
// 19M threads × ~780 GB/frame memory traffic, hitting ~2 sec/frame at
// 4400² and tripping AE's per-frame timeout (517 errors). step1 adds
// metadata-driven early-out for flat regions and bounds the scan by a
// runtime cap (env var SMOOTH_GPU_MAX_LENGTH, default 32) to bring the
// per-frame time within AE's tolerance. CPU equivalence at the GPU
// profile = "cap + GPU mode set" is enforced separately in step2.
kernel void smooth_per_pixel(
    device const float4* src            [[buffer(0)]],  // BGRA128 input
    device float4*       dst            [[buffer(1)]],  // BGRA128 output (every pixel written)
    device const uchar*  metadata       [[buffer(2)]],  // 1 byte/pixel mode_flg + fast_compare
    constant uint&       src_pitch      [[buffer(3)]],
    constant uint&       dst_pitch      [[buffer(4)]],
    constant uint&       width          [[buffer(5)]],
    constant uint&       height         [[buffer(6)]],
    constant uint&       logical_width  [[buffer(7)]],
    constant float&      range          [[buffer(8)]],
    constant uint&       white_opt      [[buffer(9)]],
    constant float&      line_weight    [[buffer(10)]],
    constant uint&       gpu_max_length [[buffer(11)]],
    uint2                gid            [[thread_position_in_grid]])
{
    if (gid.x >= width || gid.y >= height) return;
    const int px = int(gid.x);
    const int py = int(gid.y);
    const int cap = int(gpu_max_length);
    const int width_i  = int(width);
    const int height_i = int(height);

    // Default output: src[me] with white-strip applied (passthrough case).
    const float4 src_me = src[uint(py) * src_pitch + uint(px)];
    const float4 out_default = load_strip(src_me, white_opt);

    // ===== Phase 0: cap-range cardinal early-out scan =====
    // Read metadata for self + 4 cardinal directions up to `cap`.
    // If NO position (self or candidate) shows mode_flg=15, the output
    // pixel cannot be touched by any line blend AND isn't itself a
    // mode15 inside → src copy.
    //
    // Self flat alone is NOT sufficient (a centre within cap could write
    // to me via line blend) — the cap-range cardinal scan is the
    // tightest correct condition, per Hiroshi 2026-05-05 review.
    const uchar m_self = metadata[uint(py) * width + uint(px)];
    bool any_mode15 = metadata_is_mode15(m_self);
    if (!any_mode15) {
        for (int k = 1; k <= cap; k++) {
            const int xp = px + k;
            if (xp >= width_i) break;
            if (metadata_is_mode15(metadata[uint(py) * width + uint(xp)])) { any_mode15 = true; break; }
        }
    }
    if (!any_mode15) {
        for (int k = 1; k <= cap; k++) {
            const int xn = px - k;
            if (xn < 0) break;
            if (metadata_is_mode15(metadata[uint(py) * width + uint(xn)])) { any_mode15 = true; break; }
        }
    }
    if (!any_mode15) {
        for (int k = 1; k <= cap; k++) {
            const int yp = py + k;
            if (yp >= height_i) break;
            if (metadata_is_mode15(metadata[uint(yp) * width + uint(px)])) { any_mode15 = true; break; }
        }
    }
    if (!any_mode15) {
        for (int k = 1; k <= cap; k++) {
            const int yn = py - k;
            if (yn < 0) break;
            if (metadata_is_mode15(metadata[uint(yn) * width + uint(px)])) { any_mode15 = true; break; }
        }
    }
    if (!any_mode15) {
        dst[uint(py) * dst_pitch + uint(px)] = out_default;
        return;
    }

    // ===== Phase 1/2/3: full per-output writer selection (cap-bounded) =====
    float4 out = out_default;

    WriterCandidate best;
    best.found = false;
    best.ref_off_x = 0;
    best.ref_off_y = 0;
    best.t = 0;
    best.last = 0;
    best.len = 0.0f;

    // ===== Phase 1: Block 2 (cy > py) — LATER than self-inside =====
    // Centres at (px, py+1+k). Scan k=cap-1 down (largest cy first).
    for (int k = cap - 1; k >= 0; k--) {
        const int cx = px;
        const int cy = py + 1 + k;
        if (cy < 1 || uint(cy) >= height) continue;
        WriterCandidate trial = try_block_candidate(
            best, src, metadata, src_pitch,
            cx, cy, k, /*block_id*/ 2,
            /*flg1*/ -1, 0, 1u,
            /*flg2*/  1, 0, 2u,
            /*block_fire_mask*/ 0x3u,
            /*target*/ cx, cy - 1,
            /*step*/   0, -1,
            /*scan*/   1, int(height) - 2, cy,
            width, height, logical_width,
            range, white_opt, line_weight, cap);
        if (trial.found) { best = trial; break; }
    }

    // ===== Phase 1: Block 1 (cy=py, cx > px) — LATER than self-inside =====
    // Centres at (px+1+k, py). Scan k=cap-1 down (largest cx first).
    if (!best.found) {
        for (int k = cap - 1; k >= 0; k--) {
            const int cx = px + 1 + k;
            const int cy = py;
            if (uint(cx) >= width) continue;
            WriterCandidate trial = try_block_candidate(
                best, src, metadata, src_pitch,
                cx, cy, k, /*block_id*/ 1,
                /*flg1*/ 0, -1, 1u,
                /*flg2*/ 0,  1, 8u,
                /*block_fire_mask*/ 0x9u,
                /*target*/ cx - 1, cy,
                /*step*/   -1, 0,
                /*scan*/   1, int(width) - 2, cx,
                width, height, logical_width,
                range, white_opt, line_weight, cap);
            if (trial.found) { best = trial; break; }
        }
    }

    // ===== Phase 2: My own inside (mode_flg=15 4-corner avg) =====
    // Only relevant when no later outside writer exists. Inside is later
    // than block 3 / block 4 candidates so it dominates them. Step1
    // uses metadata for the mode_flg=15 check and only falls into the
    // corner-flg compute when necessary.
    bool   is_inside  = false;
    uint   inside_flg = 0u;
    if (!best.found && metadata_is_mode15(m_self)
        && px >= 1 && py >= 1 && uint(px) + 1u < logical_width && uint(py) + 1u < height) {
        inside_flg = compute_centre_corner_flg_only(
            src, src_pitch, uint(px), uint(py), range, white_opt);
        is_inside = true;
    }

    // ===== Phase 3: Block 3 (cy=py, cx<px) then Block 4 (cy<py) =====
    // Only when no later outside AND not self-inside.
    if (!best.found && !is_inside) {
        // Block 3: centres at (px-1-k, py). Scan k=0 up (largest cx
        // within block 3 = closest to me).
        for (int k = 0; k < cap; k++) {
            const int cx = px - 1 - k;
            const int cy = py;
            if (cx < 1) break;
            WriterCandidate trial = try_block_candidate(
                best, src, metadata, src_pitch,
                cx, cy, k, /*block_id*/ 3,
                /*flg1*/ 0, -1, 2u,
                /*flg2*/ 0,  1, 4u,
                /*block_fire_mask*/ 0x6u,
                /*target*/ cx + 1, cy,
                /*step*/   1, 0,
                /*scan*/   1, int(width) - 2, cx,
                width, height, logical_width,
                range, white_opt, line_weight, cap);
            if (trial.found) { best = trial; break; }
        }
        // Block 4: centres at (px, py-1-k). Scan k=0 up (largest cy
        // within block 4 = closest above me).
        if (!best.found) {
            for (int k = 0; k < cap; k++) {
                const int cx = px;
                const int cy = py - 1 - k;
                if (cy < 1) break;
                WriterCandidate trial = try_block_candidate(
                    best, src, metadata, src_pitch,
                    cx, cy, k, /*block_id*/ 4,
                    /*flg1*/  1, 0, 4u,
                    /*flg2*/ -1, 0, 8u,
                    /*block_fire_mask*/ 0xcu,
                    /*target*/ cx, cy + 1,
                    /*step*/   0, 1,
                    /*scan*/   1, int(height) - 2, cy,
                    width, height, logical_width,
                    range, white_opt, line_weight, cap);
                if (trial.found) { best = trial; break; }
            }
        }
    }

    // ===== Output =====
    if (best.found) {
        // Outside writer (block 2/1/3/4): blend_line value at iter t.
        float pre_ratio;
        if (best.t == 0) {
            pre_ratio = 0.0f;
        } else {
            const int t_prev = best.t - 1;
            const float l_prev = best.len - float(best.last - t_prev);
            pre_ratio = (l_prev * l_prev * 0.25f) / best.len;
        }
        const float l = best.len - float(best.last - best.t);
        const float ratio = (l * l * 0.25f) / best.len;
        const float r = 1.0f - (ratio - pre_ratio);

        const int rx = px + best.ref_off_x;
        const int ry = py + best.ref_off_y;
        if (rx >= 0 && ry >= 0 && uint(rx) < width && uint(ry) < height) {
            const float4 a = load_strip(src[uint(py) * src_pitch + uint(px)], white_opt);
            const float4 b = load_strip(src[uint(ry) * src_pitch + uint(rx)], white_opt);
            out = blending_pixel_f(a, b, r);
        }
        // else: out keeps default (load_strip(src_me)) — defensive
    } else if (is_inside) {
        // Inside (mode_flg=15 self): 4-corner avg per link8_square_execute.
        const float4 c  = load_strip(src[uint(py)        * src_pitch + uint(px)], white_opt);
        const float4 ul = load_strip(src[uint(py - 1)    * src_pitch + uint(px - 1)], white_opt);
        const float4 ur = load_strip(src[uint(py - 1)    * src_pitch + uint(px + 1)], white_opt);
        const float4 br = load_strip(src[uint(py + 1)    * src_pitch + uint(px + 1)], white_opt);
        const float4 bl = load_strip(src[uint(py + 1)    * src_pitch + uint(px - 1)], white_opt);
        const float4 t_ul = ((inside_flg & 1u) != 0u) ? c : blending_pixel_f(c, ul, 0.5f);
        const float4 t_ur = ((inside_flg & 2u) != 0u) ? c : blending_pixel_f(c, ur, 0.5f);
        const float4 t_br = ((inside_flg & 4u) != 0u) ? c : blending_pixel_f(c, br, 0.5f);
        const float4 t_bl = ((inside_flg & 8u) != 0u) ? c : blending_pixel_f(c, bl, 0.5f);
        out = (t_ul + t_ur + t_br + t_bl) * 0.25f;
    }
    // else: out = identity (load_strip(src_me)), already initialised above

    dst[uint(py) * dst_pitch + uint(px)] = out;
}

// ========================================================================
// C-2.5b.2-prep2a: smooth_blend kernel — partial port.
//
// For each output pixel, read the centre pixel from `src` (post-preprocess
// intermediate) plus the modes byte from the detect pass. If the pixel
// triggers mode_flg == 15 (= every cardinal neighbour different = isolated
// pixel surrounded by other colours), apply the CENTRE-PIXEL part of
// `link8_square_execute` from link8.rs:
//
//     for each of 4 diagonals D:
//         if compare_pixel_equal(centre, D)  → temp[i] = centre
//         else                                → temp[i] = blend(centre, D, 0.5)
//     out = average(temp[0..4])
//
// All other mode_flg values (3, 5, 7, 11, 13, etc) currently fall through
// to identity copy — the line-level blends from link8_square_blend_outside,
// up_mode_*_blending, down_mode_*_blending, lack_mode_* land in subsequent
// prep steps. They have inter-thread write conflicts that need careful
// algorithm inversion (each thread writes only its own pixel by reading
// from any neighbour whose decision could affect it), so they are NOT in
// this commit.
//
// `src` and `dst` are separate buffers so this kernel has NO read-after-
// write hazard: src is the immutable post-preprocess image, dst gets the
// final output. The chain dispatcher allocates the intermediate in
// dispatch_smooth_chain and threads the buffers through.
//
// `modes` pitch is `width` (1 byte per pixel, no padding) per detect kernel
// convention.
kernel void smooth_blend(
    device const float4* src           [[buffer(0)]],  // BGRA128 post-preprocess
    device float4*       dst           [[buffer(1)]],  // BGRA128 output
    device const uchar*  modes         [[buffer(2)]],
    constant uint&       src_pitch     [[buffer(3)]],  // pixels
    constant uint&       dst_pitch     [[buffer(4)]],  // pixels
    constant uint&       width         [[buffer(5)]],
    constant uint&       height        [[buffer(6)]],
    constant uint&       logical_width [[buffer(7)]],
    constant float&      range         [[buffer(8)]],
    uint2                gid           [[thread_position_in_grid]])
{
    if (gid.x >= width || gid.y >= height) return;
    const uint x = gid.x;
    const uint y = gid.y;

    const float4 c = src[y * src_pitch + x];

    // Default: identity copy. mode_flg < 15 / outside inner region / etc.
    float4 out = c;

    // Inner region only — same gate the detect kernel applied.
    if (x >= 1u && x + 1u < logical_width && y >= 1u && y + 1u < height) {
        const uchar m = modes[y * width + x];
        if ((m & 0x80u) != 0u) {
            const uint mode_flg = (uint)(m & 0x0Fu);
            if (mode_flg == 15u) {
                // Read 4 diagonals — same offsets as link8.rs ref_tbl:
                //   [0] (x-1, y-1) upper-left
                //   [1] (x+1, y-1) upper-right
                //   [2] (x+1, y+1) lower-right
                //   [3] (x-1, y+1) lower-left
                const float4 d_ul = src[(y - 1u) * src_pitch + (x - 1u)];
                const float4 d_ur = src[(y - 1u) * src_pitch + (x + 1u)];
                const float4 d_br = src[(y + 1u) * src_pitch + (x + 1u)];
                const float4 d_bl = src[(y + 1u) * src_pitch + (x - 1u)];

                // For each diagonal: if it's the same colour as centre
                // (within tolerance), keep centre as-is; otherwise blend
                // centre with the diagonal at 0.5. Then average the four.
                const float4 t_ul = compare_pixel_equal(c, d_ul, range) ? c : blending_pixel_f(c, d_ul, 0.5f);
                const float4 t_ur = compare_pixel_equal(c, d_ur, range) ? c : blending_pixel_f(c, d_ur, 0.5f);
                const float4 t_br = compare_pixel_equal(c, d_br, range) ? c : blending_pixel_f(c, d_br, 0.5f);
                const float4 t_bl = compare_pixel_equal(c, d_bl, range) ? c : blending_pixel_f(c, d_bl, 0.5f);

                out = (t_ul + t_ur + t_br + t_bl) * 0.25f;
            }
            // mode_flg ∈ {3, 5, 7, 11, 13}: TODO in prep2b+. Identity for now.
        }
    }

    dst[y * dst_pitch + x] = out;
}
