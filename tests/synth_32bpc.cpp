// Phase 2-A.2 Step 4b — synthetic 32bpc goldens generator (AE-free).
//
// What this does
// --------------
// Takes one SMDP v1 input.raw from tests/goldens/v1.4.0-ae2025/ (8 or
// 16 bpc), promotes its pixels into the AE 32bpc f32 domain, runs
// smooth_core::process<PF_PixelFloat>, and writes the matching SMDP v2
// 32bpc {in,out}.raw pair into the requested output directory.
//
// Why no AE
// ---------
// The CPU 32bpc implementation itself is the reference for v1.6.0-32bpc
// goldens (no independent oracle). With that license,
// driving the capture from existing v1.4.0 inputs sidesteps two
// blockers we discovered late in Step 4b prep: (a) the v1.4.0 .aep
// project file was never committed and one of its source layers
// (frame 135, 2512×1412) is unrecoverable; (b) AE projects are global-
// depth, so reproducing the mixed 8/16bpc set in a single 32bpc
// session is impossible. EXR-based capture (tests/capture_32bpc.py +
// tests/CAPTURE_32BPC_RUNBOOK.md) remains in the tree as the documented
// alternative path for future HDR test material from AE; this tool is
// the pragmatic primary path for the 14-frame regression suite.
//
// Promotion math
// --------------
// 8bpc: f32 = u8 / 255.0f. The smoothing range scales the same way:
// range_f32 = u32_range / 255 (so the normalized threshold ratio
// matches the integer-domain run).
// 16bpc: f32 = u16 / 32768.0f (AE PF_Pixel16 uses 0x8000 as max, not
// 0xFFFF). range_f32 = u32_range / 32768.
//
// Pixel layout: PF_Pixel8 / PF_Pixel16 / PF_PixelFloat all use the
// same {alpha, red, green, blue} struct order, so promotion is a
// straight scalar map without any channel reordering.
//
// Usage
// -----
//   synth_32bpc <v1.4.0_input.raw> <output_dir>
//
// Side effects
// ------------
//   <output_dir>/frame_NNNN_in.raw   (SMDP v2, bpc=32)
//   <output_dir>/frame_NNNN_out.raw  (SMDP v2, bpc=32)
//
// Exit codes
//   0  pair written
//   2  cannot read source raw
//   3  unsupported source bpc
//   4  output write failed

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "AEConfig.h"
#include "AE_Effect.h"
#include "A.h"

#include "smooth_core.h"

struct SourceDump {
    uint32_t version = 1;
    uint32_t width = 0;
    uint32_t height = 0;
    uint32_t bpc = 0;
    uint32_t rowbytes = 0;
    uint32_t frame_n = 0;
    uint32_t range_u32 = 0;
    float    line_weight = 0.5f;
    uint32_t white = 0;
    std::vector<uint8_t> pixels;
};

static bool read_smdp_source(const std::string& path, SourceDump& d) {
    FILE* f = std::fopen(path.c_str(), "rb");
    if (!f) { std::fprintf(stderr, "synth_32bpc: cannot open %s\n", path.c_str()); return false; }
    uint8_t hdr[64];
    if (std::fread(hdr, 1, 64, f) != 64) { std::fclose(f); return false; }
    if (std::memcmp(hdr, "SMDP", 4) != 0) { std::fclose(f); return false; }
    std::memcpy(&d.version,    hdr + 4,  4);
    std::memcpy(&d.width,      hdr + 8,  4);
    std::memcpy(&d.height,     hdr + 12, 4);
    std::memcpy(&d.bpc,        hdr + 16, 4);
    std::memcpy(&d.rowbytes,   hdr + 20, 4);
    std::memcpy(&d.frame_n,    hdr + 28, 4);
    std::memcpy(&d.range_u32,  hdr + 32, 4);
    std::memcpy(&d.line_weight,hdr + 36, 4);
    std::memcpy(&d.white,      hdr + 40, 4);
    const size_t nbytes = (size_t)d.rowbytes * (size_t)d.height;
    d.pixels.resize(nbytes);
    if (std::fread(d.pixels.data(), 1, nbytes, f) != nbytes) {
        std::fclose(f); return false;
    }
    std::fclose(f);
    return true;
}

static void write_smdp_v2_32bpc(const std::string& path,
                                uint32_t width, uint32_t height,
                                uint32_t frame_n, float range_f32,
                                float line_weight, uint32_t white,
                                const float* pixels_f32) {
    FILE* f = std::fopen(path.c_str(), "wb");
    if (!f) { std::fprintf(stderr, "synth_32bpc: cannot write %s\n", path.c_str()); std::exit(4); }
    uint8_t hdr[64] = {0};
    std::memcpy(hdr, "SMDP", 4);
    uint32_t version = 2;
    uint32_t bpc = 32;
    uint32_t rowbytes = width * 16;
    uint32_t channels = 4;
    uint32_t range_u32 = 0;  // unused on 32bpc path
    std::memcpy(hdr + 4,  &version,    4);
    std::memcpy(hdr + 8,  &width,      4);
    std::memcpy(hdr + 12, &height,     4);
    std::memcpy(hdr + 16, &bpc,        4);
    std::memcpy(hdr + 20, &rowbytes,   4);
    std::memcpy(hdr + 24, &channels,   4);
    std::memcpy(hdr + 28, &frame_n,    4);
    std::memcpy(hdr + 32, &range_u32,  4);
    std::memcpy(hdr + 36, &line_weight,4);
    std::memcpy(hdr + 40, &white,      4);
    std::memcpy(hdr + 44, &range_f32,  4);
    // reserved[4] (offset 48..63) stays zero
    std::fwrite(hdr, 1, 64, f);
    const size_t nbytes = (size_t)rowbytes * (size_t)height;
    std::fwrite(pixels_f32, 1, nbytes, f);
    std::fclose(f);
}

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: %s <v1.4.0_input.raw> <output_dir>\n", argv[0]);
        return 1;
    }
    SourceDump src;
    if (!read_smdp_source(argv[1], src)) return 2;
    if (src.bpc != 8 && src.bpc != 16) {
        std::fprintf(stderr, "synth_32bpc: unsupported source bpc=%u (need 8 or 16)\n", src.bpc);
        return 3;
    }

    const uint32_t W = src.width;
    const uint32_t H = src.height;
    const uint32_t out_rowbytes = W * 16;
    const size_t   out_nbytes   = (size_t)out_rowbytes * (size_t)H;

    // Promote to f32 ARGB. Source rows may be padded (rowbytes > W*pxsize);
    // the output is densely packed so the regression harness sees rowbytes
    // == width * 16. Both v1.4.0 fixtures we have today are unpadded, but
    // honour the rowbytes field anyway to stay correct under future layouts.
    std::vector<float> in_f32(out_nbytes / sizeof(float));
    const float scale = (src.bpc == 8) ? (1.0f / 255.0f) : (1.0f / 32768.0f);
    const size_t src_pxsize = (src.bpc == 8) ? 4 : 8;
    for (uint32_t y = 0; y < H; y++) {
        const uint8_t* sr = src.pixels.data() + (size_t)y * src.rowbytes;
        float*         dr = in_f32.data()    + (size_t)y * (out_rowbytes / sizeof(float));
        for (uint32_t x = 0; x < W; x++) {
            if (src.bpc == 8) {
                const uint8_t* p = sr + x * 4;
                dr[x*4 + 0] = p[0] * scale;  // alpha
                dr[x*4 + 1] = p[1] * scale;  // red
                dr[x*4 + 2] = p[2] * scale;  // green
                dr[x*4 + 3] = p[3] * scale;  // blue
            } else {
                const uint16_t* p = reinterpret_cast<const uint16_t*>(sr + x * 8);
                dr[x*4 + 0] = p[0] * scale;
                dr[x*4 + 1] = p[1] * scale;
                dr[x*4 + 2] = p[2] * scale;
                dr[x*4 + 3] = p[3] * scale;
            }
        }
    }

    // Equivalent f32 sum threshold: keep the same normalized "same-color"
    // tolerance the integer-domain run had. 8bpc range_u32 has units of
    // (sum-of-channel-deltas in u8), so dividing by 255 puts it in the
    // [0, 4] range that PF_PixelFloat deltas live in.
    const float scale_range = (src.bpc == 8) ? (1.0f / 255.0f) : (1.0f / 32768.0f);
    const float range_f32 = src.range_u32 * scale_range;

    smooth_core::Params p{};
    p.range        = 0;            // unused on 32bpc path
    p.range_f32    = range_f32;
    p.line_weight  = src.line_weight;
    p.white_option = (src.white != 0);

    std::vector<float> out_f32 = in_f32;  // PF_COPY equivalent

    smooth_core::process<PF_PixelFloat>(
        reinterpret_cast<PF_PixelFloat*>(in_f32.data()),
        reinterpret_cast<PF_PixelFloat*>(out_f32.data()),
        (int)W, (int)H, (int)out_rowbytes, p);

    char out_in_path[1024];
    char out_out_path[1024];
    std::snprintf(out_in_path,  sizeof(out_in_path),  "%s/frame_%04u_in.raw",  argv[2], src.frame_n);
    std::snprintf(out_out_path, sizeof(out_out_path), "%s/frame_%04u_out.raw", argv[2], src.frame_n);

    write_smdp_v2_32bpc(out_in_path,  W, H, src.frame_n, range_f32, src.line_weight, src.white, in_f32.data());
    write_smdp_v2_32bpc(out_out_path, W, H, src.frame_n, range_f32, src.line_weight, src.white, out_f32.data());

    std::printf("synth frame=%u %ux%u src_bpc=%u range_u32=%u -> range_f32=%.6g lw=%.4f white=%u\n",
                src.frame_n, W, H, src.bpc, src.range_u32, range_f32, src.line_weight, src.white);
    return 0;
}
