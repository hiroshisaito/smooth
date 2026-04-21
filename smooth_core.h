// smooth-mod-v1.5.0 Step 3/4: AE SDK 非依存のコア処理
//
// 旧 Effect.cpp::smoothing() の本体部分(白抜き + 領域検出 + メイン走査ループ)を
// ここに純関数 smooth_core::process<PixelType>() として切り出した。
//
// Step 4: 行ループ並列化
//   SMOOTH_PARALLEL=1 (デフォルト) で std::thread による行ブロック並列化を有効化。
//   SMOOTH_PARALLEL=0 でシリアル動作(回帰比較用)。
//   現状は naive な行ブロック分割。境界書き込み衝突の可能性があり、回帰テストで確認が必要。

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
#include "upMode.h"
#include "downMode.h"
#include "Lack.h"
#include "8link.h"
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
// 8bpc (sizeof==4) → smooth_core_preprocess_u8、16bpc (sizeof==8) → _u16。
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

// 行範囲 [j_start, j_end) をスキャン+ブレンドする。
// blend_info は値渡し(各スレッド独立)。in_ptr / out_ptr の buffer 自体は共有(out_ptr は共有書き込み対象)。
template <typename PixelType>
inline void process_row_range(BlendingInfo<PixelType> blend_info,
                              int j_start, int j_end,
                              int i_start, int i_end) {
    // FAST_COMPARE_PIXEL マクロは `in_ptr` と `PackedPixelType` をローカルスコープに要求する
    using PackedPixelType = typename std::conditional<sizeof(PixelType) == 4,
                                                      std::uint32_t,
                                                      std::uint64_t>::type;
    PixelType* const in_ptr        = blend_info.in_ptr;
    const int        in_width      = blend_info.width;
    const int        out_width     = in_width;
    const int        logical_width = blend_info.logical_width;
    (void)out_width;

    BlendingInfo<PixelType>* info = &blend_info;

    float weight;
    bool  lack_flg;
    long  in_target, out_target;

    for (int j = j_start; j < j_end; j++) {
        lack_flg = false;

        in_target  = (long)j * in_width  + i_start;
        out_target = (long)j * out_width + i_start;

        for (int i = i_start; i < i_end; i++, in_target++, out_target++) {
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

// メイン走査 + ブレンド処理。in_ptr 側は preProcess で白抜き済みの前提。
// 呼び出し側は事前に in_ptr 全体を out_ptr にコピーしておくこと(PF_COPY 相当)。
template <typename PixelType>
inline void process(PixelType* in_ptr, PixelType* out_ptr,
                    int logical_width, int height, int rowbytes,
                    const Params& p) {
    const int in_width = (int)(rowbytes / sizeof(PixelType));

    // 1) 白抜きと境界検出(シーケンシャル、1 パス)
    int eh_top, eh_left, eh_right, eh_bottom;
    preProcess<PixelType>(in_ptr, rowbytes, height,
                          &eh_top, &eh_left, &eh_right, &eh_bottom,
                          p.white_option);

    // 2) 領域調整(端を 1px 内側に寄せる)
    if (eh_top == 0)                eh_top = 1;
    if (eh_left == 0)               eh_left = 1;
    if (eh_right == logical_width)  eh_right -= 1;
    if (eh_bottom == height)        eh_bottom -= 1;

    // 3) BlendingInfo テンプレートを作成(以降はコピーを各スレッドに渡す)
    BlendingInfo<PixelType> tmpl;
    tmpl.in_ptr        = in_ptr;
    tmpl.out_ptr       = out_ptr;
    tmpl.width         = in_width;
    tmpl.logical_width = logical_width;
    tmpl.height        = height;
    tmpl.rowbytes      = rowbytes;
    tmpl.range         = p.range;
    tmpl.LineWeight    = p.line_weight;

    if (eh_bottom <= eh_top || eh_right <= eh_left) {
        return;  // 何も処理しない領域
    }

#if SMOOTH_PARALLEL
    // 4) 行方向ブロック並列化
    //
    // 衝突リスク: 角部のブレンドは out_ptr に j±length 行まで書き込むので、
    // 隣接スレッドの書き込み領域が重なる可能性がある。
    // 対策: 並列フェーズの後にシーケンシャルな「シーム再パス」をかけ、
    //       ストリップ境界近傍を上書きしてシリアル結果と一致させる。
    //       ブレンドは in_ptr (preProcess 後不変) から毎回再計算されるため、
    //       再処理は冪等(同じ入力 → 同じ出力)。
    // SEAM_HALO は各境界の上下何行を sequential 再処理するか。
    //
    // 計測結果(8 コア, HD 16bpc 1920x1080):
    //   halo=0   (無修復) : 5.8 ms / 3.3x speedup / 境界で ~30 bytes 差 (0.0003%)
    //   halo=32  : 20 ms  / 差はほぼゼロだが不安定(thread schedule 依存)
    //   halo=64  : 33 ms  / byte-identical / ただしシリアル 19ms より遅い
    //   halo=128 : 53 ms  / byte-identical / ただしシリアル 19ms より遅い
    //
    // → シーム修復の sequential コストが並列化の利得を上回る。
    //   Phase 1 では halo=0 で最大スループットを取り、境界残差(invisible level)を
    //   許容誤差として受け入れる。厳密 byte-identical が必要な用途は SMOOTH_PARALLEL=0 で
    //   シリアルフォールバックを使用できる。
    constexpr int SEAM_HALO = 0;

    int nthreads = (int)std::thread::hardware_concurrency();
    if (nthreads <= 0) nthreads = 1;

    const int rows = eh_bottom - eh_top;
    // しきい値: 小さい画像やコア数が少ない場合はシリアル
    if (nthreads <= 1 || rows < 32) {
        process_row_range<PixelType>(tmpl, eh_top, eh_bottom, eh_left, eh_right);
        return;
    }

    const int rows_per_thread = (rows + nthreads - 1) / nthreads;
    std::vector<std::thread> workers;
    workers.reserve(nthreads);
    std::vector<std::pair<int,int>> strips;  // 実行した strip 範囲の記録
    strips.reserve(nthreads);
    for (int t = 0; t < nthreads; t++) {
        const int start = eh_top + t * rows_per_thread;
        const int end   = std::min(start + rows_per_thread, eh_bottom);
        if (start >= end) break;
        strips.emplace_back(start, end);
        workers.emplace_back([tmpl, start, end, eh_left, eh_right]() {
            process_row_range<PixelType>(tmpl, start, end, eh_left, eh_right);
        });
    }
    for (auto& w : workers) w.join();

    // 5) シーム再パス: 隣接 strip 境界を halo 分シーケンシャル再処理
    //    strip[k].end == strip[k+1].start が境界。
    //    この境界を囲む [boundary - halo, boundary + halo) を上書き。
    for (size_t k = 1; k < strips.size(); k++) {
        const int boundary = strips[k].first;
        const int seam_start = std::max(eh_top,    boundary - SEAM_HALO);
        const int seam_end   = std::min(eh_bottom, boundary + SEAM_HALO);
        if (seam_start < seam_end) {
            process_row_range<PixelType>(tmpl, seam_start, seam_end, eh_left, eh_right);
        }
    }
#else
    process_row_range<PixelType>(tmpl, eh_top, eh_bottom, eh_left, eh_right);
#endif
}

} // namespace smooth_core

#endif // SMOOTH_CORE_H_
