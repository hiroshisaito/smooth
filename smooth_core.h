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

#include <algorithm>
#include <cstdint>
#include <cstring>
#include <cstddef>
#include <thread>
#include <type_traits>
#include <vector>

#include "define.h"
#include "util.h"
#include "smooth_core_ffi.h"

#ifndef SMOOTH_PARALLEL
#define SMOOTH_PARALLEL 1
#endif

namespace smooth_core {

struct Params {
    unsigned int range;        // 同色判定しきい値 (bpc 依存スケール済)
    float        line_weight;  // 0.5..1.0
    bool         white_option; // 白抜き実施
};

// Phase 2-C Step 2: preProcess は Rust FFI に委譲。
template <typename PixelType>
inline void preProcess(PixelType* in_ptr, int rowbytes, int height,
                       int* out_top, int* out_left, int* out_right, int* out_bottom,
                       bool is_white_trans) {
    smooth_bbox_t bbox = {0, 0, 0, 0};
    if constexpr (sizeof(PixelType) == 4) {
        smooth_core_preprocess_u8(reinterpret_cast<void*>(in_ptr),
                                  rowbytes, height,
                                  is_white_trans ? 1 : 0, &bbox);
    } else {
        static_assert(sizeof(PixelType) == 8, "preProcess only supports 8bpc (4B) or 16bpc (8B) pixels");
        smooth_core_preprocess_u16(reinterpret_cast<void*>(in_ptr),
                                   rowbytes, height,
                                   is_white_trans ? 1 : 0, &bbox);
    }
    *out_top    = bbox.top;
    *out_left   = bbox.left;
    *out_right  = bbox.right;
    *out_bottom = bbox.bottom;
}

// Phase 2-C Step 3: process_row_range は Rust 実装(FFI 1 本)。
// Rust 側で FAST_COMPARE / ComparePixel / 角パターン処理 / ブレンドまで完結する。
template <typename PixelType>
inline void invoke_row_range_ffi(PixelType* in_ptr, PixelType* out_ptr,
                                 int width, int logical_width, int height, int rowbytes,
                                 unsigned int range, float line_weight,
                                 int j_start, int j_end, int i_start, int i_end) {
    smooth_row_range_args_t args;
    args.in_ptr        = reinterpret_cast<void*>(in_ptr);
    args.out_ptr       = reinterpret_cast<void*>(out_ptr);
    args.width         = width;
    args.logical_width = logical_width;
    args.height        = height;
    args.rowbytes      = rowbytes;
    args.range         = range;
    args.line_weight   = line_weight;
    args.j_start       = j_start;
    args.j_end         = j_end;
    args.i_start       = i_start;
    args.i_end         = i_end;

    if constexpr (sizeof(PixelType) == 4) {
        smooth_core_process_row_range_u8(&args);
    } else {
        static_assert(sizeof(PixelType) == 8, "row_range supports 8bpc (4B) or 16bpc (8B) only");
        smooth_core_process_row_range_u16(&args);
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

#if SMOOTH_PARALLEL
    // Phase 1 と同じ行ブロック並列化(SEAM_HALO=0)。境界修復は Step 4 で rayon に移設して再設計。
    constexpr int SEAM_HALO = 0;

    int nthreads = (int)std::thread::hardware_concurrency();
    if (nthreads <= 0) nthreads = 1;

    const int rows = eh_bottom - eh_top;
    if (nthreads <= 1 || rows < 32) {
        invoke_row_range_ffi<PixelType>(in_ptr, out_ptr,
                                         in_width, logical_width, height, rowbytes,
                                         p.range, p.line_weight,
                                         eh_top, eh_bottom, eh_left, eh_right);
        return;
    }

    const int rows_per_thread = (rows + nthreads - 1) / nthreads;
    std::vector<std::thread> workers;
    workers.reserve(nthreads);
    std::vector<std::pair<int,int>> strips;
    strips.reserve(nthreads);
    for (int t = 0; t < nthreads; t++) {
        const int start = eh_top + t * rows_per_thread;
        const int end   = std::min(start + rows_per_thread, eh_bottom);
        if (start >= end) break;
        strips.emplace_back(start, end);
        workers.emplace_back([=]() {
            invoke_row_range_ffi<PixelType>(in_ptr, out_ptr,
                                             in_width, logical_width, height, rowbytes,
                                             p.range, p.line_weight,
                                             start, end, eh_left, eh_right);
        });
    }
    for (auto& w : workers) w.join();

    for (size_t k = 1; k < strips.size(); k++) {
        const int boundary = strips[k].first;
        const int seam_start = std::max(eh_top,    boundary - SEAM_HALO);
        const int seam_end   = std::min(eh_bottom, boundary + SEAM_HALO);
        if (seam_start < seam_end) {
            invoke_row_range_ffi<PixelType>(in_ptr, out_ptr,
                                             in_width, logical_width, height, rowbytes,
                                             p.range, p.line_weight,
                                             seam_start, seam_end, eh_left, eh_right);
        }
    }
#else
    invoke_row_range_ffi<PixelType>(in_ptr, out_ptr,
                                     in_width, logical_width, height, rowbytes,
                                     p.range, p.line_weight,
                                     eh_top, eh_bottom, eh_left, eh_right);
#endif
}

} // namespace smooth_core

#endif // SMOOTH_CORE_H_
