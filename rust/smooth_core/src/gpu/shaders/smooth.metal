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

// C-2.5b.2-prep2b.2 (CreateGPUWorld variant): smooth_priority_init.
//
// Zero out the two priority intermediate buffers that the follow-up
// claim/apply kernels use for line-level blend write conflict
// resolution. Each pixel's priority slot is initialised to UINT32_MAX
// so atomic_min() in the claim kernel reduces to "lowest source-i-index
// that touched this pixel" without needing a separate "untouched"
// sentinel.
//
// Priority buffers are now allocated by the C++ side via
// gpu_suite->CreateGPUWorld with PF_PixelFormat_GPU_BGRA128 — the
// SDK-canonical pattern for compute-kernel intermediates. We interpret
// the underlying memory as `device uint*` and use only the FIRST
// uint32 of each BGRA128 pixel (4 uints per pixel) — the other 3 are
// unused. To address pixel (x, y)'s priority slot:
//   slot_index = y * (priority_pitch_pixels * 4) + x * 4
// where priority_pitch_pixels is the BGRA128 row stride in pixels
// (= AE-reported rowbytes / 16). This is the row stride the C++ side
// pulls from priority_world->rowbytes.
//
// `priority_v` tracks vertical-line claims; `priority_h` tracks
// horizontal-line claims.
kernel void smooth_priority_init(
    device uint*   priority_v             [[buffer(0)]],
    device uint*   priority_h             [[buffer(1)]],
    constant uint& priority_pitch_pixels  [[buffer(2)]],
    constant uint& width                  [[buffer(3)]],
    constant uint& height                 [[buffer(4)]],
    uint2          gid                    [[thread_position_in_grid]])
{
    if (gid.x >= width || gid.y >= height) return;
    const uint idx = gid.y * (priority_pitch_pixels * 4u) + gid.x * 4u;
    priority_v[idx] = 0xFFFFFFFFu;
    priority_h[idx] = 0xFFFFFFFFu;
    // Other 3 uints of this BGRA pixel slot left untouched (unused).
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
    uint white_opt)
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

    // Bounded loop: SMOOTH_MAX_LENGTH iterations cap divergence between
    // threads even if the CPU bounding logic would have allowed more.
    for (int iter = 0; iter < SMOOTH_MAX_LENGTH; iter++) {
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
// ========================================================================
// C-2.5b.2-prep2b.2b: smooth_blend_mode15_outside_{claim,apply} kernels.
//
// Port of `link8_square_blend_outside` (link8.rs:390-405) per design memo
// `docs/PHASE_2A_PREP2B_DESIGN_MEMO.md` §6 prep2b.2.
//
// 2026-05-04 RETRY note: an earlier implementation (commit ac408f7) used
// 19M-thread monolithic claim/apply dispatches. Real-device UAT failed
// with AE warning "smooth did not render anything" + multiple
// `FrameTask threw 517` log entries — symptoms of a GPU driver
// watchdog timeout (~2s/dispatch) being intermittently exceeded by
// mode_flg=15-dense regions. That commit was reverted (commit 3cea31b);
// this implementation uses **tile dispatch** (option Path 1 in workbench
// 2026-05-04 entry): the host loops over WxH-area tiles and encodes one
// dispatch_thread_groups per tile within the same command buffer.
// `tile_origin` constant tells the kernel its absolute pixel offset.
// Workload per tile is bounded (TILE×TILE × per-thread cost), well under
// the watchdog. Atomic semantics across tiles are preserved (Metal
// serialises dispatches within a single compute encoder + command
// buffer).
//
// CPU semantics:
//
//     // After centre 4-corner-avg already written by smooth_combined,
//     // link8_square_execute issues up to 4 outside-line blend calls per
//     // centre with mode_flg=15. Each call:
//     //   count = count_length_two_lines(...)
//     //   blend_line(count, in_target, out_target, ref_offset,
//     //              step_in, step_out, ratio_invert=true, no_line_weight=t0_flg)
//
// GPU strategy (option (b), bit-identical via atomic claim+apply):
//   Pass A (claim, tiled): each centre with mode_flg=15 walks its 4
//                          active outside calls and atomic_fetch_min on
//                          the relevant priority buffer (priority_h for
//                          horizontal lines, priority_v for vertical).
//   Pass B (apply, tiled): each centre re-walks the same calls; for each
//                          touched output pixel, reads priority and
//                          writes blend only if `priority[idx] ==
//                          this centre's i_index`.
//
// "Lowest i_index wins" matches CPU scan order (row-major, ascending
// j then i): the FIRST centre to call link8_square_blend_outside on a
// given output pixel wins. UAT will determine if this differs visibly
// from CPU "last writer wins" semantics; if so we flip atomic_min →
// atomic_max (init=0).

// Compute the blend ratio for output pixel index t along a line of total
// `len_pixels` pixels with effective length `len`. Mirrors CPU
// `blend_line` (blend.rs:62-98) inner loop. `pre_ratio` is updated by
// caller across iterations; `ratio_invert` matches CPU's bool param.
inline float blend_line_ratio(
    int   t,
    float len,
    int   len_pixels_minus_1,
    float pre_ratio,
    bool  ratio_invert)
{
    const float l = len - float(len_pixels_minus_1 - t);
    const float ratio = (l * l * 0.25f) / len;
    const float diff  = ratio - pre_ratio;
    return ratio_invert ? (1.0f - diff) : diff;
}

// Port of `blending_pixel_f` (blend.rs:15-45) for f32/Pixel32, max=1.0.
// Channel layout: BGRA on GPU. The CPU-side branches collapse to:
//   - both alpha == 1.0: lerp RGB, alpha = 1.0
//   - target.alpha == 0: return ref.rgb, blended alpha
//   - ref.alpha    == 0: return target.rgb, blended alpha
//   - else: lerp RGB and alpha
inline float4 blending_pixel_f_full(float4 target, float4 ref_p, float ratio) {
    const float r_alpha = 1.0f - ratio;
    const float ta = target.w;
    const float ra = ref_p.w;
    const float out_a = ta * ratio + ra * r_alpha;
    if (ta == 1.0f && ra == 1.0f) {
        return float4(
            target.x * ratio + ref_p.x * r_alpha,
            target.y * ratio + ref_p.y * r_alpha,
            target.z * ratio + ref_p.z * r_alpha,
            1.0f);
    }
    if (ta == 0.0f) {
        return float4(ref_p.x, ref_p.y, ref_p.z, out_a);
    }
    if (ra == 0.0f) {
        return float4(target.x, target.y, target.z, out_a);
    }
    return float4(
        target.x * ratio + ref_p.x * r_alpha,
        target.y * ratio + ref_p.y * r_alpha,
        target.z * ratio + ref_p.z * r_alpha,
        out_a);
}

// Compute the 4-corner equality flg for the centre at (x, y), or 0xFF
// if (x, y) is not a mode_flg=15 centre (early-out sentinel). Used by
// both claim and apply kernels — same inner-region gate, mode_flg test,
// and 4-diagonal compare_pixel_equal as link8_square_execute L416-420.
inline uint compute_centre_flg_15(
    device const float4* src,
    uint src_pitch,
    uint x, uint y,
    uint logical_width, uint height,
    float range,
    uint white_opt)
{
    if (x < 1u || x + 1u >= logical_width || y < 1u || y + 1u >= height) {
        return 0xFFu;
    }
    const float4 c     = load_strip(src[y * src_pitch + x], white_opt);
    const float4 right = load_strip(src[y * src_pitch + (x + 1u)], white_opt);
    if (!fast_compare_pixel(c, right)) return 0xFFu;

    const float4 up   = load_strip(src[(y - 1u) * src_pitch + x], white_opt);
    const float4 down = load_strip(src[(y + 1u) * src_pitch + x], white_opt);
    const float4 left = load_strip(src[y * src_pitch + (x - 1u)], white_opt);

    uint mode_flg = 0u;
    if (compare_pixel(c, right, range)) mode_flg |= 1u;
    if (compare_pixel(c, up,    range)) mode_flg |= 2u;
    if (compare_pixel(c, down,  range)) mode_flg |= 4u;
    if (compare_pixel(c, left,  range)) mode_flg |= 8u;
    if (mode_flg != 15u) return 0xFFu;

    const float4 ul = load_strip(src[(y - 1u) * src_pitch + (x - 1u)], white_opt);
    const float4 ur = load_strip(src[(y - 1u) * src_pitch + (x + 1u)], white_opt);
    const float4 br = load_strip(src[(y + 1u) * src_pitch + (x + 1u)], white_opt);
    const float4 bl = load_strip(src[(y + 1u) * src_pitch + (x - 1u)], white_opt);

    uint flg = 0u;
    if (compare_pixel_equal(c, ul, range)) flg |= 1u;
    if (compare_pixel_equal(c, ur, range)) flg |= 2u;
    if (compare_pixel_equal(c, br, range)) flg |= 4u;
    if (compare_pixel_equal(c, bl, range)) flg |= 8u;
    return flg;
}

// dst index for separate-pitch buffers.
inline uint dst_idx_for(int x, int y, uint dst_pitch) {
    return uint(y) * dst_pitch + uint(x);
}

// Run claim or apply for a single outside-line call. Caller passes the
// CPU-side parameters of `link8_square_blend_outside` decomposed into
// per-axis ints. `is_apply = false` for claim phase (atomic_min only);
// `true` for apply (read priority + conditionally write blend).
//
// Bounds: count_length_two_lines's min/max/limit_from_here arguments
// keep the line scan within [0, in_w/h - 1]. We additionally guard
// every per-pixel write/read with explicit bounds checks before any
// buffer access — defensive against rounding edges in line_weight
// scaling and against bugs in min/max derivation.
inline void process_outside_line(
    device const float4* src,
    device float4*       dst,
    device atomic_uint*  priority_v,
    device atomic_uint*  priority_h,
    uint src_pitch,
    uint dst_pitch,
    uint priority_pitch_pixels,
    uint width,
    uint height,
    int  target_x, int target_y,
    int  ref_off_x, int ref_off_y,
    int  step_x,    int step_y,
    int  min_bound, int max_bound, int limit_from_here,
    float range,
    uint  white_opt,
    float line_weight,
    uint  centre_index,
    bool  axis_h,
    bool  is_apply)
{
    const CountLenResult clr = count_length_two_lines(
        src, src_pitch,
        target_x, target_y,
        target_x + ref_off_x, target_y + ref_off_y,
        step_x, step_y,
        min_bound, max_bound, limit_from_here,
        range, white_opt);
    if (clr.length <= 0) return;

    const float lw  = clr.t0_flg ? 0.5f : line_weight;
    const float len = float(clr.length) * lw;
    const int   len_pixels = int(ceil(len));
    if (len_pixels <= 0) return;
    const int   last = len_pixels - 1;

    device atomic_uint* priority = axis_h ? priority_h : priority_v;
    // BGRA128 stride: 4 uints per pixel; only the first uint per pixel
    // is used as the priority slot.
    const uint priority_row_uints = priority_pitch_pixels * 4u;

    // CPU blend_line walks t=0..last writing pixels in REVERSE order
    // along step (start = target + last*step, decrement by step each iter).
    // Output pixel for iter t is at target + (last - t) * step.
    float pre_ratio = 0.0f;
    for (int t = 0; t < len_pixels; ++t) {
        const int offset = last - t;
        const int px_x = target_x + offset * step_x;
        const int px_y = target_y + offset * step_y;
        // Bounds guard: stop on any OOB. CPU's min/max derivation
        // should keep us in range, but defend anyway.
        if (px_x < 0 || px_y < 0) break;
        if (uint(px_x) >= width || uint(px_y) >= height) break;

        // priority_pitch_pixels is in BGRA128 pixels (4 uints each); we
        // store the priority value in the FIRST uint of each pixel slot.
        const uint out_idx = uint(px_y) * priority_row_uints + uint(px_x) * 4u;
        if (is_apply) {
            const uint winner = atomic_load_explicit(&priority[out_idx], memory_order_relaxed);
            if (winner == centre_index) {
                const float r = blend_line_ratio(t, len, last, pre_ratio, true);
                const int rx = px_x + ref_off_x;
                const int ry = px_y + ref_off_y;
                if (rx >= 0 && ry >= 0
                 && uint(rx) < width && uint(ry) < height) {
                    const float4 a = src[uint(px_y) * src_pitch + uint(px_x)];
                    const float4 b = src[uint(ry) * src_pitch + uint(rx)];
                    const float4 out_pixel = blending_pixel_f_full(a, b, r);
                    dst[dst_idx_for(px_x, px_y, dst_pitch)] = out_pixel;
                }
            }
        } else {
            atomic_fetch_min_explicit(&priority[out_idx], centre_index, memory_order_relaxed);
        }

        // Update pre_ratio for next iteration (CPU does this regardless
        // of whether the per-iter blend write happened).
        const float l = len - float(last - t);
        pre_ratio = (l * l * 0.25f) / len;
    }
}

// Run all 4 outside conditional blocks for a centre with mode_flg=15.
// Mirrors link8_square_execute L448-502 precisely.
inline void run_outside_blocks(
    device const float4* src,
    device float4*       dst,
    device atomic_uint*  priority_v,
    device atomic_uint*  priority_h,
    uint src_pitch,
    uint dst_pitch,
    uint priority_pitch_pixels,
    uint width,
    uint height,
    int  cx, int cy,
    uint flg,
    float range,
    uint  white_opt,
    float line_weight,
    uint  centre_index,
    bool  is_apply)
{
    const int in_w = int(width);
    const int in_h = int(height);

    // Block 1: flg & 0x9 != 0x9 → horizontal line through (cx-1, cy)
    if ((flg & 0x9u) != 0x9u) {
        if ((flg & 1u) != 0u) {
            process_outside_line(
                src, dst, priority_v, priority_h, src_pitch, dst_pitch,
                priority_pitch_pixels, width, height,
                cx - 1, cy, 0, -1, -1, 0,
                1, in_w - 2, cx, range, white_opt, line_weight,
                centre_index, /*axis_h*/ true, is_apply);
        } else if ((flg & 8u) != 0u) {
            process_outside_line(
                src, dst, priority_v, priority_h, src_pitch, dst_pitch,
                priority_pitch_pixels, width, height,
                cx - 1, cy, 0, 1, -1, 0,
                1, in_w - 2, cx, range, white_opt, line_weight,
                centre_index, /*axis_h*/ true, is_apply);
        }
    }

    // Block 2: flg & 0x3 != 0x3 → vertical line through (cx, cy-1)
    if ((flg & 0x3u) != 0x3u) {
        if ((flg & 1u) != 0u) {
            process_outside_line(
                src, dst, priority_v, priority_h, src_pitch, dst_pitch,
                priority_pitch_pixels, width, height,
                cx, cy - 1, -1, 0, 0, -1,
                1, in_h - 2, cy, range, white_opt, line_weight,
                centre_index, /*axis_h*/ false, is_apply);
        } else if ((flg & 2u) != 0u) {
            process_outside_line(
                src, dst, priority_v, priority_h, src_pitch, dst_pitch,
                priority_pitch_pixels, width, height,
                cx, cy - 1, 1, 0, 0, -1,
                1, in_h - 2, cy, range, white_opt, line_weight,
                centre_index, /*axis_h*/ false, is_apply);
        }
    }

    // Block 3: flg & 0x6 != 0x6 → horizontal line through (cx+1, cy)
    if ((flg & 0x6u) != 0x6u) {
        if ((flg & 2u) != 0u) {
            process_outside_line(
                src, dst, priority_v, priority_h, src_pitch, dst_pitch,
                priority_pitch_pixels, width, height,
                cx + 1, cy, 0, -1, 1, 0,
                1, in_w - 2, cx, range, white_opt, line_weight,
                centre_index, /*axis_h*/ true, is_apply);
        } else if ((flg & 4u) != 0u) {
            process_outside_line(
                src, dst, priority_v, priority_h, src_pitch, dst_pitch,
                priority_pitch_pixels, width, height,
                cx + 1, cy, 0, 1, 1, 0,
                1, in_w - 2, cx, range, white_opt, line_weight,
                centre_index, /*axis_h*/ true, is_apply);
        }
    }

    // Block 4: flg & 0xc != 0xc → vertical line through (cx, cy+1)
    if ((flg & 0xcu) != 0xcu) {
        if ((flg & 4u) != 0u) {
            process_outside_line(
                src, dst, priority_v, priority_h, src_pitch, dst_pitch,
                priority_pitch_pixels, width, height,
                cx, cy + 1, 1, 0, 0, 1,
                1, in_h - 2, cy, range, white_opt, line_weight,
                centre_index, /*axis_h*/ false, is_apply);
        } else if ((flg & 8u) != 0u) {
            process_outside_line(
                src, dst, priority_v, priority_h, src_pitch, dst_pitch,
                priority_pitch_pixels, width, height,
                cx, cy + 1, -1, 0, 0, 1,
                1, in_h - 2, cy, range, white_opt, line_weight,
                centre_index, /*axis_h*/ false, is_apply);
        }
    }
}

// Pass A: claim phase, tiled. Each thread = candidate centre pixel
// within the tile starting at `tile_origin`. If mode_flg=15, atomic_min
// the touched output pixels' priority slots.
//
// The host loops over WxH-area tiles and encodes one dispatch per tile.
// `gid` is local to the tile; absolute pixel coords = `gid + tile_origin`.
kernel void smooth_blend_mode15_outside_claim(
    device const float4* src                   [[buffer(0)]],
    device float4*       dst                   [[buffer(1)]],
    device atomic_uint*  priority_v            [[buffer(2)]],
    device atomic_uint*  priority_h            [[buffer(3)]],
    constant uint&       src_pitch             [[buffer(4)]],
    constant uint&       dst_pitch             [[buffer(5)]],
    constant uint&       priority_pitch_pixels [[buffer(6)]],
    constant uint&       width                 [[buffer(7)]],
    constant uint&       height                [[buffer(8)]],
    constant uint&       logical_width         [[buffer(9)]],
    constant float&      range                 [[buffer(10)]],
    constant uint&       white_opt             [[buffer(11)]],
    constant float&      line_weight           [[buffer(12)]],
    constant uint2&      tile_origin           [[buffer(13)]],
    uint2                gid                   [[thread_position_in_grid]])
{
    const uint x = gid.x + tile_origin.x;
    const uint y = gid.y + tile_origin.y;
    if (x >= width || y >= height) return;

    const uint flg = compute_centre_flg_15(src, src_pitch, x, y,
                                            logical_width, height, range, white_opt);
    if (flg == 0xFFu) return;

    const uint centre_index = y * width + x;
    run_outside_blocks(
        src, dst, priority_v, priority_h,
        src_pitch, dst_pitch, priority_pitch_pixels, width, height,
        int(x), int(y), flg,
        range, white_opt, line_weight, centre_index,
        /*is_apply*/ false);
}

// Pass B: apply phase, tiled. Same per-thread structure as claim, but
// reads priority and writes blend conditionally.
kernel void smooth_blend_mode15_outside_apply(
    device const float4* src                   [[buffer(0)]],
    device float4*       dst                   [[buffer(1)]],
    device atomic_uint*  priority_v            [[buffer(2)]],
    device atomic_uint*  priority_h            [[buffer(3)]],
    constant uint&       src_pitch             [[buffer(4)]],
    constant uint&       dst_pitch             [[buffer(5)]],
    constant uint&       priority_pitch_pixels [[buffer(6)]],
    constant uint&       width                 [[buffer(7)]],
    constant uint&       height                [[buffer(8)]],
    constant uint&       logical_width         [[buffer(9)]],
    constant float&      range                 [[buffer(10)]],
    constant uint&       white_opt             [[buffer(11)]],
    constant float&      line_weight           [[buffer(12)]],
    constant uint2&      tile_origin           [[buffer(13)]],
    uint2                gid                   [[thread_position_in_grid]])
{
    const uint x = gid.x + tile_origin.x;
    const uint y = gid.y + tile_origin.y;
    if (x >= width || y >= height) return;

    const uint flg = compute_centre_flg_15(src, src_pitch, x, y,
                                            logical_width, height, range, white_opt);
    if (flg == 0xFFu) return;

    const uint centre_index = y * width + x;
    run_outside_blocks(
        src, dst, priority_v, priority_h,
        src_pitch, dst_pitch, priority_pitch_pixels, width, height,
        int(x), int(y), flg,
        range, white_opt, line_weight, centre_index,
        /*is_apply*/ true);
}

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
