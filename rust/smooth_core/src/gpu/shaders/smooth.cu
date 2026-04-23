// CUDA shader stub. Sub-stage E replaces this with the real 2-pass
// smooth implementation. For Sub-stage B we only need a syntactically
// valid file so build.rs can reference it later.

extern "C" __global__ void smooth_placeholder(
    const float4* __restrict__ src,
    float4*       __restrict__ dst,
    int                         n)
{
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) dst[i] = src[i];
}
