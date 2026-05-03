// MSL kernels for the smooth GPU path.
//
// AE delivers GPU effect worlds in PF_PixelFormat_GPU_BGRA128 (per
// AE_EffectPixelFormat.h L41), so float4 components map to:
//   x = blue, y = green, z = red, w = alpha
// This is the OPPOSITE channel order from PF_PixelFloat on the CPU side
// (which is ARGB: x=alpha, y=red, z=green, w=blue). The MSL code below
// uses .x/.y/.z = b/g/r consistently; do not import names from the CPU
// helpers without remapping.

#include <metal_stdlib>
using namespace metal;

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
