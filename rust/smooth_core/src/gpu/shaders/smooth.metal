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
