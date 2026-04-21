// smooth-mod-v1.5.0 Step 3: AE SDK 非依存のコア処理
//
// 旧 Effect.cpp::smoothing() の本体部分(白抜き + 領域検出 + メイン走査ループ)を
// ここに純関数 smooth_core::process<PixelType>() として切り出した。
//
// 並列化/SIMD はまだ行わず、1.4.0-ae2025 と pixel-by-pixel で同一結果を保証する。

#ifndef SMOOTH_CORE_H_
#define SMOOTH_CORE_H_

#include <cstdint>
#include <cstring>
#include <cstddef>
#include <type_traits>

#include "define.h"
#include "util.h"
#include "upMode.h"
#include "downMode.h"
#include "Lack.h"
#include "8link.h"

namespace smooth_core {

struct Params {
    unsigned int range;        // 同色判定しきい値 (bpc 依存スケール済)
    float        line_weight;  // 0.5..1.0
    bool         white_option; // 白抜き実施
};

// 白/透明ピクセル生成(旧 Effect.cpp の static inline を移設)
inline void getWhitePixel(PF_Pixel16* white)  { PF_Pixel16 c = {0x8000, 0x8000, 0x8000, 0x8000}; *white = c; }
inline void getWhitePixel(PF_Pixel8*  white)  { PF_Pixel8  c = {0xFF, 0xFF, 0xFF, 0xFF};         *white = c; }
inline void getNullPixel (PF_Pixel16* null_p) { PF_Pixel16 c = {0, 0, 0, 0};                     *null_p = c; }
inline void getNullPixel (PF_Pixel8*  null_p) { PF_Pixel8  c = {0, 0, 0, 0};                     *null_p = c; }

// 白抜きと境界検出。in_ptr は in-place 書き換え(白色ピクセルを null pixel に)。
// rect の top/left/right/bottom は出力される領域(half-open ではない、end は含む側を +1 した値)。
template <typename PixelType>
inline void preProcess(PixelType* in_ptr, int rowbytes, int height,
                       int* out_top, int* out_left, int* out_right, int* out_bottom,
                       bool is_white_trans) {
    PixelType key;
    PixelType null_pixel;
    smooth_core::getWhitePixel(&key);
    smooth_core::getNullPixel(&null_pixel);

    const int width = (int)(rowbytes / sizeof(PixelType));

    int  top = 0, left = width, right = 0, bottom = 0;
    bool top_found = false, left_found = false;

    if (is_white_trans) {
        int t = 0;
        for (int j = 0; j < height; j++) {
            if (!top_found) top = j;
            for (int i = 0; i < width; i++) {
                if (key.red == in_ptr[t].red &&
                    key.green == in_ptr[t].green &&
                    key.blue == in_ptr[t].blue) {
                    in_ptr[t] = null_pixel;
                } else if (in_ptr[t].alpha == 0) {
                    // already transparent
                } else {
                    top_found = true;
                    left_found = true;
                    if (left > i) left = i;
                    if (right < i) right = i;
                    if (bottom < j) bottom = j;
                }
                t++;
            }
        }
    } else {
        int t = 0;
        for (int j = 0; j < height; j++) {
            if (!top_found) top = j;
            for (int i = 0; i < width; i++) {
                if (!(key.red == in_ptr[t].red && key.green == in_ptr[t].green && key.blue == in_ptr[t].blue) &&
                    in_ptr[t].alpha != 0) {
                    top_found = true;
                    left_found = true;
                    if (left > i) left = i;
                    if (right < i) right = i;
                    if (bottom < j) bottom = j;
                }
                t++;
            }
        }
    }

    *out_top    = top_found  ? top  : 0;
    *out_left   = left_found ? left : 0;
    *out_right  = right + 1;
    *out_bottom = bottom + 1;
}

// メイン走査 + ブレンド処理。in_ptr 側は preProcess で白抜き済みの前提。
// 呼び出し側は事前に in_ptr 全体を out_ptr にコピーしておくこと(PF_COPY 相当)。
template <typename PixelType>
inline void process(PixelType* in_ptr, PixelType* out_ptr,
                    int logical_width, int height, int rowbytes,
                    const Params& p) {
    // FAST_COMPARE_PIXEL マクロ用の typedef(4 byte -> uint32, 8 byte -> uint64)
    using PackedPixelType = typename std::conditional<sizeof(PixelType) == 4,
                                                      std::uint32_t,
                                                      std::uint64_t>::type;
    const int in_width  = (int)(rowbytes / sizeof(PixelType));
    const int out_width = in_width;  // in/out 同サイズ前提
    (void)out_width;

    // 1) 白抜きと境界検出
    int eh_top, eh_left, eh_right, eh_bottom;
    preProcess<PixelType>(in_ptr, rowbytes, height,
                          &eh_top, &eh_left, &eh_right, &eh_bottom,
                          p.white_option);

    // 2) 領域調整(端を 1px 内側に寄せる)
    if (eh_top == 0)                eh_top = 1;
    if (eh_left == 0)               eh_left = 1;
    if (eh_right == logical_width)  eh_right -= 1;
    if (eh_bottom == height)        eh_bottom -= 1;

    // 3) BlendingInfo セットアップ
    BlendingInfo<PixelType> blend_info;
    BlendingInfo<PixelType>* info = &blend_info;
    blend_info.in_ptr        = in_ptr;
    blend_info.out_ptr       = out_ptr;
    blend_info.width         = in_width;
    blend_info.logical_width = logical_width;
    blend_info.height        = height;
    blend_info.rowbytes      = rowbytes;
    blend_info.range         = p.range;
    blend_info.LineWeight    = p.line_weight;

    // 4) アンチ処理本体
    float weight;
    bool  lack_flg;
    long  in_target, out_target;

    for (int j = eh_top; j < eh_bottom; j++) {
        lack_flg = false;

        in_target  = (long)j * in_width  + eh_left;
        out_target = (long)j * out_width + eh_left;

        for (int i = eh_left; i < eh_right; i++, in_target++, out_target++) {
            if (lack_flg) {
                lack_flg = false;
                blend_info.i          = i;
                blend_info.j          = j;
                blend_info.in_target  = in_target;
                blend_info.out_target = out_target;
                blend_info.flag       = 0;
                LackMode0304Execute(&blend_info);
            }

            if (FAST_COMPARE_PIXEL(in_target, in_target + 1)) {
                unsigned char mode_flg = 0;

                blend_info.i          = i;
                blend_info.j          = j;
                blend_info.in_target  = in_target;
                blend_info.out_target = out_target;
                blend_info.flag       = 0;

                std::memset(&blend_info.core, 0, sizeof(Cinfo) * 4);

                if (ComparePixel(in_target, in_target + 1))           (mode_flg |= 1 << 0);
                if (ComparePixel(in_target, in_target - in_width))    (mode_flg |= 1 << 1);
                if (ComparePixel(in_target, in_target + in_width))    (mode_flg |= 1 << 2);
                if (ComparePixel(in_target, in_target - 1))           (mode_flg |= 1 << 3);

                if (mode_flg != 0) {
                    if (i < logical_width - 2 && (mode_flg & 1 << 0)) {
                        lack_flg = true;
                    }

                    switch (mode_flg) {
                    case 3: // 上向きの角
                        if (ComparePixelEqual(in_target - in_width,     in_target + 1) &&
                            ComparePixel     (in_target - in_width + 1, in_target - in_width) &&
                            ComparePixel     (in_target - in_width + 1, in_target + 1)) {
                            break;
                        }

                        upMode_LeftCountLength<PixelType>(&blend_info);
                        upMode_RightCountLength<PixelType>(&blend_info);
                        upMode_TopCountLength<PixelType>(&blend_info);
                        upMode_BottomCountLength<PixelType>(&blend_info);

                        if (blend_info.core[0].length - blend_info.core[1].length == 1) {
                            blend_info.core[0].start -= 0.5f;
                            blend_info.core[1].start -= 0.5f;
                        }
                        weight = ((blend_info.core[0].flg & CR_FLG_FILL) ||
                                  (blend_info.core[1].flg & CR_FLG_FILL))
                                     ? 0.5f
                                     : blend_info.LineWeight;
                        blend_info.core[0].end = blend_info.core[0].start - (blend_info.core[0].start - blend_info.core[0].end) * weight;
                        blend_info.core[1].end = blend_info.core[1].start + (blend_info.core[1].end   - blend_info.core[1].start) * weight;

                        if (blend_info.core[3].length - blend_info.core[2].length == 1) {
                            blend_info.core[2].start += 0.5f;
                            blend_info.core[3].start += 0.5f;
                        }
                        weight = ((blend_info.core[2].flg & CR_FLG_FILL) ||
                                  (blend_info.core[3].flg & CR_FLG_FILL))
                                     ? 0.5f
                                     : blend_info.LineWeight;
                        blend_info.core[2].end = blend_info.core[2].start - (blend_info.core[2].start - blend_info.core[2].end) * weight;
                        blend_info.core[3].end = blend_info.core[3].start + (blend_info.core[3].end   - blend_info.core[3].start) * weight;

                        if (blend_info.core[0].length >= 2 && blend_info.core[3].length >= 2) {
                            LackMode02Execute(&blend_info);
                        } else if (blend_info.core[1].length > 0) {
                            blend_info.mode = BLEND_MODE_UP_H;
                            upMode_LeftBlending<PixelType>(&blend_info);
                            upMode_RightBlending<PixelType>(&blend_info);
                            if (blend_info.core[2].length > 1) {
                                upMode_TopBlending<PixelType>(&blend_info);
                                upMode_BottomBlending<PixelType>(&blend_info);
                            }
                        } else if (blend_info.core[2].length > 0) {
                            blend_info.mode = BLEND_MODE_UP_V;
                            upMode_TopBlending<PixelType>(&blend_info);
                            upMode_BottomBlending<PixelType>(&blend_info);
                        }
                        break;

                    case 5: // 下向きの角
                        if (ComparePixelEqual(in_target + in_width,     in_target + 1) &&
                            ComparePixel     (in_target + in_width + 1, in_target + in_width) &&
                            ComparePixel     (in_target + in_width + 1, in_target + 1)) {
                            break;
                        }

                        downMode_LeftCountLength<PixelType>(&blend_info);
                        downMode_RightCountLength<PixelType>(&blend_info);
                        downMode_TopCountLength<PixelType>(&blend_info);
                        downMode_BottomCountLength<PixelType>(&blend_info);

                        if (blend_info.core[0].length - blend_info.core[1].length == 1) {
                            blend_info.core[0].start -= 0.5f;
                            blend_info.core[1].start -= 0.5f;
                        }
                        weight = ((blend_info.core[0].flg & CR_FLG_FILL) ||
                                  (blend_info.core[1].flg & CR_FLG_FILL))
                                     ? 0.5f
                                     : blend_info.LineWeight;
                        blend_info.core[0].end = blend_info.core[0].start - (blend_info.core[0].start - blend_info.core[0].end) * weight;
                        blend_info.core[1].end = blend_info.core[1].start + (blend_info.core[1].end   - blend_info.core[1].start) * weight;

                        if (blend_info.core[3].length - blend_info.core[2].length == 1) {
                            blend_info.core[2].start += 0.5f;
                            blend_info.core[3].start += 0.5f;
                        }
                        weight = ((blend_info.core[2].flg & CR_FLG_FILL) ||
                                  (blend_info.core[3].flg & CR_FLG_FILL))
                                     ? 0.5f
                                     : blend_info.LineWeight;
                        blend_info.core[2].end = blend_info.core[2].start - (blend_info.core[2].start - blend_info.core[2].end) * weight;
                        blend_info.core[3].end = blend_info.core[3].start + (blend_info.core[3].end   - blend_info.core[3].start) * weight;

                        if (blend_info.core[0].length >= 2 && blend_info.core[2].length >= 2) {
                            LackMode01Execute(&blend_info);
                        } else if (blend_info.core[1].length > 0) {
                            blend_info.mode = BLEND_MODE_UP_H;
                            downMode_LeftBlending<PixelType>(&blend_info);
                            downMode_RightBlending<PixelType>(&blend_info);
                            if (blend_info.core[3].length > 1) {
                                downMode_TopBlending<PixelType>(&blend_info);
                                downMode_BottomBlending<PixelType>(&blend_info);
                            }
                        } else if (blend_info.core[3].length > 0) {
                            blend_info.mode = BLEND_MODE_UP_V;
                            downMode_TopBlending<PixelType>(&blend_info);
                            downMode_BottomBlending<PixelType>(&blend_info);
                        }
                        break;

                    case 7:  Link8Mode01Execute(&blend_info); break;
                    case 11: Link8Mode02Execute(&blend_info); break;
                    case 13: Link8Mode04Execute(&blend_info); break;
                    case 15: Link8SquareExecute(&blend_info); break;
                    default: break;
                    }

                    // 突起mode3
                    if (i < logical_width - 2) {
                        blend_info.i          = i + 1;
                        blend_info.j          = j;
                        blend_info.in_target  = in_target + 1;
                        blend_info.out_target = out_target + 1;
                        blend_info.flag       = 0;

                        mode_flg = 0;
                        if (ComparePixel(blend_info.in_target, blend_info.in_target - in_width)) (mode_flg |= 1 << 0);
                        if (ComparePixel(blend_info.in_target, blend_info.in_target + in_width)) (mode_flg |= 1 << 1);
                        if (ComparePixel(blend_info.in_target, blend_info.in_target + 1))        (mode_flg |= 1 << 2);

                        if (3 == mode_flg) {
                            Link8Mode03Execute(&blend_info);
                        }
                    }
                }
            }
        }
    }
}

} // namespace smooth_core

#endif // SMOOTH_CORE_H_
