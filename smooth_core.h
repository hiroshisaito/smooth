// smooth-mod Phase 2-C: AE SDK 非依存のコア処理
//
// Phase 1: C++ でコア関数化、行ブロック並列化。
// Phase 2-C Step 2: preProcess を Rust FFI に移行。
// Phase 2-C Step 3: process_row_range 本体(スキャン/ブレンド)と全ヘルパを Rust に集約し、
//   C++ 側は 1 行の FFI 呼び出しのみ。std::thread 並列化の枠組みは残してあり、
//   境界挙動は Phase 1 と同一(SEAM_HALO=0、NEAR-IDENTICAL 許容)。
//   Step 4 で rayon 内部化する予定。

#ifndef SMOOTH_CORE_H_
#define SMOOTH_CORE_H_

#include <cstdint>
#include <cstring>
#include <cstddef>
#include <type_traits>

#include "define.h"
#include "util.h"
#include "smooth_core_ffi.h"

#ifndef SMOOTH_PARALLEL
#define SMOOTH_PARALLEL 1
#endif

namespace smooth_core {

struct Params {
    unsigned int range;        // 同色判定しきい値 (8/16bpc: bpc 依存スケール済 u32)
    float        range_f32;    // 同上 32bpc 版 (AE 0..1 domain × 4 channels の f32 sum)
    float        line_weight;  // 0.5..1.0
    bool         white_option; // 白抜き実施
};

// Phase 2-C Step 2 + Phase 2-A.2 Step 1: preProcess は Rust FFI に委譲。
// 16 byte = 32bpc (PF_PixelFloat、Phase 2-A.2 で追加)、8 byte = 16bpc、4 byte = 8bpc。
template <typename PixelType>
inline void preProcess(PixelType* in_ptr, int rowbytes, int height,
                       int* out_top, int* out_left, int* out_right, int* out_bottom,
                       bool is_white_trans) {
    smooth_bbox_t bbox = {0, 0, 0, 0};
    if constexpr (sizeof(PixelType) == 4) {
        smooth_core_preprocess_u8(reinterpret_cast<void*>(in_ptr),
                                  rowbytes, height,
                                  is_white_trans ? 1 : 0, &bbox);
    } else if constexpr (sizeof(PixelType) == 8) {
        smooth_core_preprocess_u16(reinterpret_cast<void*>(in_ptr),
                                   rowbytes, height,
                                   is_white_trans ? 1 : 0, &bbox);
    } else {
        static_assert(sizeof(PixelType) == 16, "preProcess only supports 4B (u8) / 8B (u16) / 16B (f32) pixels");
        smooth_core_preprocess_f32(reinterpret_cast<void*>(in_ptr),
                                   rowbytes, height,
                                   is_white_trans ? 1 : 0, &bbox);
    }
    *out_top    = bbox.top;
    *out_left   = bbox.left;
    *out_right  = bbox.right;
    *out_bottom = bbox.bottom;
}

// Phase 2-C Step 3 + Phase 2-A.2 Step 1: process_row_range は Rust 実装(FFI 1 本)。
// 32bpc は別 args struct (`smooth_row_range_args_f32_t`) で `range` を f32 として渡す。
template <typename PixelType>
inline void invoke_row_range_ffi(PixelType* in_ptr, PixelType* out_ptr,
                                 int width, int logical_width, int height, int rowbytes,
                                 unsigned int range_u32, float range_f32, float line_weight,
                                 int j_start, int j_end, int i_start, int i_end) {
    if constexpr (sizeof(PixelType) == 16) {
        smooth_row_range_args_f32_t args;
        args.in_ptr        = reinterpret_cast<void*>(in_ptr);
        args.out_ptr       = reinterpret_cast<void*>(out_ptr);
        args.width         = width;
        args.logical_width = logical_width;
        args.height        = height;
        args.rowbytes      = rowbytes;
        args.range         = range_f32;
        args.line_weight   = line_weight;
        args.j_start       = j_start;
        args.j_end         = j_end;
        args.i_start       = i_start;
        args.i_end         = i_end;
        args.parallel      = SMOOTH_PARALLEL ? 1 : 0;
        smooth_core_process_row_range_f32(&args);
    } else {
        smooth_row_range_args_t args;
        args.in_ptr        = reinterpret_cast<void*>(in_ptr);
        args.out_ptr       = reinterpret_cast<void*>(out_ptr);
        args.width         = width;
        args.logical_width = logical_width;
        args.height        = height;
        args.rowbytes      = rowbytes;
        args.range         = range_u32;
        args.line_weight   = line_weight;
        args.j_start       = j_start;
        args.j_end         = j_end;
        args.i_start       = i_start;
        args.i_end         = i_end;
        args.parallel      = SMOOTH_PARALLEL ? 1 : 0;

        if constexpr (sizeof(PixelType) == 4) {
            smooth_core_process_row_range_u8(&args);
        } else {
            static_assert(sizeof(PixelType) == 8, "row_range supports 4B (u8) / 8B (u16) / 16B (f32) only");
            smooth_core_process_row_range_u16(&args);
        }
    }
}

// メイン走査 + ブレンド処理。in_ptr に対して preProcess を行い、そのあとで
// out_ptr に in_ptr の内容をコピーしてからスキャン + ブレンドを走らせる。
// 呼び出し側は PF_COPY を事前に行う必要はない(本関数が in-place 前処理 +
// 内部 memcpy で等価な結果を保証する)。
//
// この関数の契約:
//   - in_ptr / out_ptr は少なくとも rowbytes * height バイトの独立した書込可能
//     バッファを指していること。
//   - 戻り時、out_ptr は smoothing 済み画像になる。in_ptr は preProcess 分の
//     in-place 書き換えが施されている(white_option 時に白 → null_pixel)。
template <typename PixelType>
inline void process(PixelType* in_ptr, PixelType* out_ptr,
                    int logical_width, int height, int rowbytes,
                    const Params& p) {
    const int in_width = (int)(rowbytes / sizeof(PixelType));

    // 1) 白抜きと境界検出(in_ptr in-place)
    int eh_top, eh_left, eh_right, eh_bottom;
    preProcess<PixelType>(in_ptr, rowbytes, height,
                          &eh_top, &eh_left, &eh_right, &eh_bottom,
                          p.white_option);

    // 2) 変更後の in_ptr を out_ptr にまるごとコピー。
    //    これで out_ptr の内部白ピクセルも透明化され、続く scan/blend は
    //    エッジだけ上書きすれば整合が取れる。
    std::memcpy(reinterpret_cast<void*>(out_ptr),
                reinterpret_cast<const void*>(in_ptr),
                static_cast<size_t>(rowbytes) * static_cast<size_t>(height));

    // 3) 領域調整(端を 1px 内側に寄せる)
    if (eh_top == 0)                eh_top = 1;
    if (eh_left == 0)               eh_left = 1;
    if (eh_right == logical_width)  eh_right -= 1;
    if (eh_bottom == height)        eh_bottom -= 1;

    if (eh_bottom <= eh_top || eh_right <= eh_left) {
        return;  // 何も処理しない領域
    }

    // 行ブロック並列化は Rust 側(rayon)で完結。C++ は FFI を 1 回呼ぶだけ。
    // SMOOTH_PARALLEL マクロで切替(回帰テストが SMOOTH_PARALLEL=0 をシリアル比較に使う)。
    invoke_row_range_ffi<PixelType>(in_ptr, out_ptr,
                                     in_width, logical_width, height, rowbytes,
                                     p.range, p.range_f32, p.line_weight,
                                     eh_top, eh_bottom, eh_left, eh_right);
}

} // namespace smooth_core

#endif // SMOOTH_CORE_H_
