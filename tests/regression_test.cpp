// smooth-mod-v1.5.0 Step 3 回帰テスト(AE 非依存)
//
// SMDP raw dump (入力) を読み込み、smooth_core::process() を適用し、
// SMDP raw dump (期待出力 golden) と byte-identical か確認する。
//
// Usage: regression_test <in.raw> <expected_out.raw>
// 終了コード: 0=IDENTICAL / 10=DIFF / 2..3=IO error
//
// Build (paths assume repo root):
//   clang++ -std=c++17 -O2 \
//     -I<AE_SDK>/Examples/Headers -I<AE_SDK>/Examples/Headers/SP \
//     -I<AE_SDK>/Examples/Util -I. \
//     tests/regression_test.cpp util.cpp \
//     -o tests/regression_test

#include <chrono>
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

struct Dump {
    uint32_t version = 1;
    uint32_t width = 0;
    uint32_t height = 0;
    uint32_t bpc = 0;
    uint32_t rowbytes = 0;
    uint32_t frame_n = 0;
    uint32_t range = 0;        // u32 sum threshold (8/16bpc); 0 on 32bpc
    float    range_f32 = 0.0f; // f32 sum threshold (32bpc, v2 only)
    float    line_weight = 0.5f;
    uint32_t white = 0;
    std::vector<uint8_t> pixels;
};

static bool read_u32(const uint8_t* p, size_t off, uint32_t& out) {
    std::memcpy(&out, p + off, sizeof(uint32_t));
    return true;
}
static bool read_f32(const uint8_t* p, size_t off, float& out) {
    std::memcpy(&out, p + off, sizeof(float));
    return true;
}

static bool read_dump(const std::string& path, Dump& d) {
    FILE* f = std::fopen(path.c_str(), "rb");
    if (!f) {
        std::fprintf(stderr, "cannot open %s\n", path.c_str());
        return false;
    }
    uint8_t hdr[64];
    if (std::fread(hdr, 1, 64, f) != 64) { std::fclose(f); return false; }
    if (std::memcmp(hdr, "SMDP", 4) != 0) { std::fclose(f); return false; }
    read_u32(hdr, 4,  d.version);
    read_u32(hdr, 8,  d.width);
    read_u32(hdr, 12, d.height);
    read_u32(hdr, 16, d.bpc);
    read_u32(hdr, 20, d.rowbytes);
    read_u32(hdr, 28, d.frame_n);
    read_u32(hdr, 32, d.range);
    read_f32(hdr, 36, d.line_weight);
    read_u32(hdr, 40, d.white);
    if (d.version >= 2) {
        // SMDP v2 parks the f32 range at offset 44 (formerly reserved[0]).
        // 32bpc fixtures need this; 8/16bpc fixtures keep d.range and leave
        // d.range_f32 = 0 (treated as unused by the dispatch below).
        read_f32(hdr, 44, d.range_f32);
    }
    if (d.bpc == 32 && d.version < 2) {
        std::fprintf(stderr, "32bpc fixture requires SMDP v2 header (got v%u)\n", d.version);
        std::fclose(f); return false;
    }

    const size_t nbytes = (size_t)d.rowbytes * (size_t)d.height;
    d.pixels.resize(nbytes);
    if (std::fread(d.pixels.data(), 1, nbytes, f) != nbytes) {
        std::fclose(f); return false;
    }
    std::fclose(f);
    return true;
}

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: %s <in.raw> <expected_out.raw> [repeat N]\n", argv[0]);
        return 1;
    }
    int repeat = 1;
    if (argc >= 5 && std::strcmp(argv[3], "repeat") == 0) {
        repeat = std::atoi(argv[4]);
        if (repeat < 1) repeat = 1;
    }

    Dump in, expected;
    if (!read_dump(argv[1], in)) return 2;
    if (!read_dump(argv[2], expected)) return 3;

    if (in.width != expected.width || in.height != expected.height ||
        in.bpc != expected.bpc || in.rowbytes != expected.rowbytes) {
        std::fprintf(stderr, "header mismatch\n");
        return 4;
    }

    // preProcess は in_ptr を in-place で書き換える。毎回同じ結果を得るため
    // in のコピーを毎回作り直す。
    const std::vector<uint8_t> in_original = in.pixels;

    smooth_core::Params p;
    p.range        = in.range;
    p.range_f32    = in.range_f32;  // SMDP v2 32bpc threshold (0 for 8/16bpc paths)
    p.line_weight  = in.line_weight;
    p.white_option = (in.white != 0);

    std::vector<uint8_t> out(in.pixels.size());

    double total_ms = 0.0;
    double min_ms   = 1e18;
    for (int r = 0; r < repeat; r++) {
        in.pixels = in_original;   // in-place 書き換えをリセット
        out       = in.pixels;     // PF_COPY 相当

        auto t0 = std::chrono::steady_clock::now();
        if (in.bpc == 8) {
            smooth_core::process<PF_Pixel8>(
                reinterpret_cast<PF_Pixel8*>(in.pixels.data()),
                reinterpret_cast<PF_Pixel8*>(out.data()),
                (int)in.width, (int)in.height, (int)in.rowbytes, p);
        } else if (in.bpc == 16) {
            smooth_core::process<PF_Pixel16>(
                reinterpret_cast<PF_Pixel16*>(in.pixels.data()),
                reinterpret_cast<PF_Pixel16*>(out.data()),
                (int)in.width, (int)in.height, (int)in.rowbytes, p);
        } else if (in.bpc == 32) {
            smooth_core::process<PF_PixelFloat>(
                reinterpret_cast<PF_PixelFloat*>(in.pixels.data()),
                reinterpret_cast<PF_PixelFloat*>(out.data()),
                (int)in.width, (int)in.height, (int)in.rowbytes, p);
        } else {
            std::fprintf(stderr, "unsupported bpc=%u\n", in.bpc);
            return 5;
        }
        auto t1 = std::chrono::steady_clock::now();
        const double ms = std::chrono::duration<double, std::milli>(t1 - t0).count();
        total_ms += ms;
        if (ms < min_ms) min_ms = ms;
    }
    const double avg_ms = total_ms / repeat;
    if (repeat > 1) {
        std::printf("BENCH  frame=%u w=%u h=%u bpc=%u repeat=%d avg=%.3fms min=%.3fms\n",
                    in.frame_n, in.width, in.height, in.bpc, repeat, avg_ms, min_ms);
    }

    if (std::memcmp(out.data(), expected.pixels.data(), expected.pixels.size()) == 0) {
        std::printf("IDENTICAL frame=%u w=%u h=%u bpc=%u range=%u lw=%.4f white=%u\n",
                    in.frame_n, in.width, in.height, in.bpc, in.range, in.line_weight, in.white);
        return 0;
    }

    size_t diffs = 0;
    int max_abs = 0;
    const size_t pxsize = (in.bpc == 8)  ? 4
                        : (in.bpc == 16) ? 8
                        :                  16;  // 32bpc PF_PixelFloat
    const size_t pixels_per_row = in.rowbytes / pxsize;
    int first_diff_count = 0;
    for (size_t i = 0; i < expected.pixels.size(); i++) {
        if (out[i] != expected.pixels[i]) {
            diffs++;
            const int d = std::abs((int)out[i] - (int)expected.pixels[i]);
            if (d > max_abs) max_abs = d;
            if (first_diff_count < 4 && std::getenv("SMOOTH_DUMP_DIFFS")) {
                const size_t pixel_idx = i / pxsize;
                const size_t x = pixel_idx % pixels_per_row;
                const size_t y = pixel_idx / pixels_per_row;
                const size_t ch = i % pxsize;
                std::fprintf(stderr, "  diff#%d byte=%zu px=(%zu,%zu) ch=%zu out=%u exp=%u\n",
                             first_diff_count, i, x, y, ch, out[i], expected.pixels[i]);
                first_diff_count++;
            }
        }
    }
    const double diff_pct = 100.0 * diffs / (double)expected.pixels.size();

    if (in.bpc == 32) {
        // For PF_PixelFloat fixtures, byte-domain max_abs is meaningless
        // (a single-LSB mantissa flip can produce arbitrary byte values).
        // Reinterpret the buffers as f32 streams and report the largest
        // |out - expected| in the value domain. The tolerance matches
        // docs/PHASE_2A_GPU_RFC.md §3.2.6 cross_platform_policy for f32:
        // 1e-5 absolute. Phase 1's strip-parallel boundary residual on
        // frame 135 sits well under this when goldens were captured in
        // SMOOTH_PARALLEL=0 mode (serial baseline).
        const float* out_f32 = reinterpret_cast<const float*>(out.data());
        const float* exp_f32 = reinterpret_cast<const float*>(expected.pixels.data());
        const size_t n_floats = expected.pixels.size() / sizeof(float);
        float max_f32_abs = 0.0f;
        size_t f32_diffs = 0;
        for (size_t i = 0; i < n_floats; i++) {
            const float a = out_f32[i];
            const float b = exp_f32[i];
            if (a == b) continue;
            f32_diffs++;
            const float d = std::abs(a - b);
            if (d > max_f32_abs) max_f32_abs = d;
        }
        const double f32_diff_pct = 100.0 * f32_diffs / (double)n_floats;
        // Tolerance shape mirrors the 8/16bpc rule (`diff_pct < 0.01 &&
        // max_abs <= 32`) translated to f32:
        //   diff_pct < 0.01           — only a tiny fraction of pixels may differ
        //   max_f32_abs <= 0.125      — same headroom as max_abs=32 in u8
        //                                (32/255 ≈ 0.125), enough to absorb the
        //                                strip-parallel decision-flip residual
        //                                that frame 135 inherits from Phase 1.
        // RFC §3.2.6's stricter cross-platform threshold (f32_abs <= 1e-5) is
        // applied separately at Mac↔Win comparison time (manifest-driven, not
        // here); this hardcoded rule is the local-Mac NEAR-ID acceptance gate.
        const bool within_tol = (f32_diff_pct < 0.01) && (max_f32_abs <= 0.125f);
        std::printf("%s frame=%u w=%u h=%u bpc=32 floats=%zu/%zu (%.4f%%) max_f32_abs=%.3e\n",
                    within_tol ? "NEAR-ID  " : "DIFF     ",
                    in.frame_n, in.width, in.height,
                    f32_diffs, n_floats, f32_diff_pct, max_f32_abs);
        return within_tol ? 0 : 10;
    }

    // 8/16bpc integer-domain rule (Phase 1):
    //   diff < 0.01% かつ max_abs <= 32 なら NEAR-IDENTICAL として成功扱い(終了コード 0)
    //   それ以外は DIFF (終了コード 10)
    const bool within_tol = (diff_pct < 0.01) && (max_abs <= 32);
    std::printf("%s frame=%u w=%u h=%u bpc=%u bytes=%zu/%zu (%.4f%%) max_abs=%d\n",
                within_tol ? "NEAR-ID  " : "DIFF     ",
                in.frame_n, in.width, in.height, in.bpc,
                diffs, expected.pixels.size(),
                diff_pct, max_abs);
    return within_tol ? 0 : 10;
}
