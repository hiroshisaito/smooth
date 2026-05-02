// Sub-stage C-1: identity passthrough kernel.
// Sub-stage C-2 will add the real 2-pass smooth (detect + blend), with
// `smooth_detect` and `smooth_blend` kernels and an intermediate buffer.

#include <metal_stdlib>
using namespace metal;

// Copies src → dst with no transformation. BGRA128 = 4×f32 per pixel,
// matching PF_PixelFormat_GPU_BGRA128 (RFC §3.3.1).
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
