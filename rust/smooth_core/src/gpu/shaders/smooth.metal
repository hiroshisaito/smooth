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
