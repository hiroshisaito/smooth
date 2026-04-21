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
    uint32_t width = 0;
    uint32_t height = 0;
    uint32_t bpc = 0;
    uint32_t rowbytes = 0;
    uint32_t frame_n = 0;
    uint32_t range = 0;
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
    read_u32(hdr, 8,  d.width);
    read_u32(hdr, 12, d.height);
    read_u32(hdr, 16, d.bpc);
    read_u32(hdr, 20, d.rowbytes);
    read_u32(hdr, 28, d.frame_n);
    read_u32(hdr, 32, d.range);
    read_f32(hdr, 36, d.line_weight);
    read_u32(hdr, 40, d.white);

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
        std::fprintf(stderr, "usage: %s <in.raw> <expected_out.raw>\n", argv[0]);
        return 1;
    }

    Dump in, expected;
    if (!read_dump(argv[1], in)) return 2;
    if (!read_dump(argv[2], expected)) return 3;

    if (in.width != expected.width || in.height != expected.height ||
        in.bpc != expected.bpc || in.rowbytes != expected.rowbytes) {
        std::fprintf(stderr, "header mismatch\n");
        return 4;
    }

    // 出力バッファは入力をコピーした状態からスタート(AE の PF_COPY 相当)
    std::vector<uint8_t> out = in.pixels;

    smooth_core::Params p;
    p.range        = in.range;
    p.line_weight  = in.line_weight;
    p.white_option = (in.white != 0);

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
    } else {
        std::fprintf(stderr, "unsupported bpc=%u\n", in.bpc);
        return 5;
    }

    if (std::memcmp(out.data(), expected.pixels.data(), expected.pixels.size()) == 0) {
        std::printf("IDENTICAL frame=%u w=%u h=%u bpc=%u range=%u lw=%.4f white=%u\n",
                    in.frame_n, in.width, in.height, in.bpc, in.range, in.line_weight, in.white);
        return 0;
    }

    size_t diffs = 0;
    int max_abs = 0;
    for (size_t i = 0; i < expected.pixels.size(); i++) {
        if (out[i] != expected.pixels[i]) {
            diffs++;
            const int d = std::abs((int)out[i] - (int)expected.pixels[i]);
            if (d > max_abs) max_abs = d;
        }
    }
    std::printf("DIFF   frame=%u w=%u h=%u bpc=%u bytes=%zu/%zu (%.3f%%) max_abs=%d\n",
                in.frame_n, in.width, in.height, in.bpc,
                diffs, expected.pixels.size(),
                100.0 * diffs / expected.pixels.size(), max_abs);
    return 10;
}
