// MSL shader stub. Sub-stage C replaces this with the real 2-pass
// smooth implementation (detect + blend). For Sub-stage B we only
// need a syntactically valid file so build.rs can reference it later.

#include <metal_stdlib>
using namespace metal;

// Placeholder identity kernel. Will be removed in Sub-stage C.
kernel void smooth_placeholder(
    device const float4* src [[buffer(0)]],
    device float4*       dst [[buffer(1)]],
    uint gid [[thread_position_in_grid]])
{
    dst[gid] = src[gid];
}
