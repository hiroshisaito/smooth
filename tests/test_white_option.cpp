// Synthetic regression for the white_option → transparent path.
// Captures the bug fixed in Phase 2-C Step 3 follow-up:
//   interior white pixels must become fully transparent in the output,
//   not only edge pixels blended by scan/blend.

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>

#include "AEConfig.h"
#include "AE_Effect.h"
#include "A.h"
#include "define.h"
#include "smooth_core.h"

template <typename PixelType>
static int run_case(const char* label, int w, int h, const PixelType& white, const PixelType& mark) {
    const int rowbytes = w * (int)sizeof(PixelType);
    std::vector<PixelType> in(w * h), out(w * h);

    // Fill everything with white, drop a non-white anchor pixel so bbox is non-empty.
    for (auto& p : in) p = white;
    in[(h / 2) * w + (w / 2)] = mark;

    smooth_core::Params p;
    p.range        = 0;
    p.line_weight  = 0.75f;
    p.white_option = true;

    // out is left uninitialized; smooth_core::process is expected to fill it entirely.
    smooth_core::process<PixelType>(in.data(), out.data(), w, h, rowbytes, p);

    // Every output pixel except the anchor should be fully transparent.
    int white_bad = 0;
    int mark_bad  = 0;
    for (int y = 0; y < h; y++) {
        for (int x = 0; x < w; x++) {
            const PixelType& op = out[y * w + x];
            const bool is_anchor = (x == w / 2 && y == h / 2);
            if (is_anchor) {
                // Anchor pixel may or may not be modified by scan/blend depending on
                // its neighbours; we only require it to stay non-null and non-white.
                if (op.alpha == 0) mark_bad++;
            } else if (op.alpha != 0) {
                white_bad++;
            }
        }
    }

    if (white_bad == 0 && mark_bad == 0) {
        std::printf("OK  %s w=%d h=%d\n", label, w, h);
        return 0;
    }
    std::printf("FAIL %s w=%d h=%d  opaque_interior_pixels=%d  anchor_lost=%d\n",
                label, w, h, white_bad, mark_bad);
    return 1;
}

int main() {
    int fails = 0;

    PF_Pixel8  w8  = {0xFF, 0xFF, 0xFF, 0xFF};
    PF_Pixel8  m8  = {0xFF, 0x20, 0x40, 0x60};
    fails += run_case<PF_Pixel8>("8bpc all-white-with-anchor",  32, 32, w8, m8);
    fails += run_case<PF_Pixel8>("8bpc all-white-large",       128, 96, w8, m8);

    PF_Pixel16 w16 = {0x8000, 0x8000, 0x8000, 0x8000};
    PF_Pixel16 m16 = {0x8000, 0x1000, 0x2000, 0x3000};
    fails += run_case<PF_Pixel16>("16bpc all-white-with-anchor", 32, 32, w16, m16);
    fails += run_case<PF_Pixel16>("16bpc all-white-large",      128, 96, w16, m16);

    return fails == 0 ? 0 : 1;
}
