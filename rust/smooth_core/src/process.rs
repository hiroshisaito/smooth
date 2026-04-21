// process_row_range — ported from smooth_core.h.
// Invoked per-row-range. Serial semantics only for this step; Step 4 adds parallelism.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Once;

use crate::compare::{compare_pixel, compare_pixel_equal, fast_compare_pixel};
use crate::down_mode::{
    down_mode_left_count_length,  down_mode_right_count_length,
    down_mode_top_count_length,   down_mode_bottom_count_length,
    down_mode_left_blending,      down_mode_right_blending,
    down_mode_top_blending,       down_mode_bottom_blending,
};
use crate::lack::{lack_mode_01_execute, lack_mode_02_execute, lack_mode_0304_execute};
use crate::link8::{
    link8_mode_01_execute, link8_mode_02_execute,
    link8_mode_03_execute, link8_mode_04_execute,
    link8_square_execute,
};
use crate::types::{BlendingInfo, Cinfo, SmoothPixel, BLEND_MODE_UP_H, BLEND_MODE_UP_V, CR_FLG_FILL};
use crate::up_mode::{
    up_mode_left_count_length,  up_mode_right_count_length,
    up_mode_top_count_length,   up_mode_bottom_count_length,
    up_mode_left_blending,      up_mode_right_blending,
    up_mode_top_blending,       up_mode_bottom_blending,
};

/// Diagnostic skip mask from SMOOTH_SKIP env var (bitmask):
/// 0x01 = skip case 3 (upMode), 0x02 = skip case 5 (downMode),
/// 0x04 = skip case 7 (link8_01), 0x08 = skip case 11 (link8_02),
/// 0x10 = skip case 13 (link8_04), 0x20 = skip case 15 (link8_square),
/// 0x40 = skip 突起 mode3 (link8_03), 0x80 = skip lack_03/04 (pre-scan path).
fn skip_mask() -> u32 {
    static INIT: Once = Once::new();
    static MASK: AtomicU32 = AtomicU32::new(0);
    INIT.call_once(|| {
        if let Ok(s) = std::env::var("SMOOTH_SKIP") {
            let v = if let Some(hex) = s.strip_prefix("0x") {
                u32::from_str_radix(hex, 16).unwrap_or(0)
            } else {
                s.parse::<u32>().unwrap_or(0)
            };
            MASK.store(v, Ordering::Relaxed);
        }
    });
    MASK.load(Ordering::Relaxed)
}

pub unsafe fn process_row_range<P: SmoothPixel>(
    template: &BlendingInfo<P>,
    j_start: i32, j_end: i32,
    i_start: i32, i_end: i32,
) {
    let skip = skip_mask();
    let mut blend_info: BlendingInfo<P> = template.clone();
    let in_width      = blend_info.width  as i64;
    let out_width     = blend_info.width  as i64;
    let logical_width = blend_info.logical_width;

    let mut weight: f32;
    let mut lack_flg: bool;

    for j in j_start..j_end {
        lack_flg = false;
        let mut in_target  = j as i64 * in_width  + i_start as i64;
        let mut out_target = j as i64 * out_width + i_start as i64;

        let mut i = i_start;
        while i < i_end {
            if lack_flg {
                lack_flg = false;
                if (skip & 0x80) == 0 {
                    blend_info.i          = i;
                    blend_info.j          = j;
                    blend_info.in_target  = in_target;
                    blend_info.out_target = out_target;
                    blend_info.flag       = 0;
                    lack_mode_0304_execute(&mut blend_info);
                }
            }

            if fast_compare_pixel(&blend_info, in_target, in_target + 1) {
                let mut mode_flg: u8 = 0;
                blend_info.i          = i;
                blend_info.j          = j;
                blend_info.in_target  = in_target;
                blend_info.out_target = out_target;
                blend_info.flag       = 0;
                blend_info.core = [Cinfo::default(); 4];

                if compare_pixel(&blend_info, in_target, in_target + 1)        { mode_flg |= 1 << 0; }
                if compare_pixel(&blend_info, in_target, in_target - in_width) { mode_flg |= 1 << 1; }
                if compare_pixel(&blend_info, in_target, in_target + in_width) { mode_flg |= 1 << 2; }
                if compare_pixel(&blend_info, in_target, in_target - 1)        { mode_flg |= 1 << 3; }

                if mode_flg != 0 {
                    if i < logical_width - 2 && (mode_flg & (1 << 0)) != 0 {
                        lack_flg = true;
                    }

                    match mode_flg {
                        3 if (skip & 0x01) == 0 => {
                            // 上向きの角
                            if compare_pixel_equal(&blend_info, in_target - in_width,     in_target + 1)
                                && compare_pixel(&blend_info, in_target - in_width + 1, in_target - in_width)
                                && compare_pixel(&blend_info, in_target - in_width + 1, in_target + 1)
                            {
                                // fallthrough to skip
                            } else {
                                up_mode_left_count_length(&mut blend_info);
                                up_mode_right_count_length(&mut blend_info);
                                up_mode_top_count_length(&mut blend_info);
                                up_mode_bottom_count_length(&mut blend_info);

                                if blend_info.core[0].length - blend_info.core[1].length == 1 {
                                    blend_info.core[0].start -= 0.5;
                                    blend_info.core[1].start -= 0.5;
                                }
                                weight = if (blend_info.core[0].flg & CR_FLG_FILL) != 0
                                         || (blend_info.core[1].flg & CR_FLG_FILL) != 0 {
                                    0.5
                                } else {
                                    blend_info.line_weight
                                };
                                blend_info.core[0].end = blend_info.core[0].start - (blend_info.core[0].start - blend_info.core[0].end) * weight;
                                blend_info.core[1].end = blend_info.core[1].start + (blend_info.core[1].end   - blend_info.core[1].start) * weight;

                                if blend_info.core[3].length - blend_info.core[2].length == 1 {
                                    blend_info.core[2].start += 0.5;
                                    blend_info.core[3].start += 0.5;
                                }
                                weight = if (blend_info.core[2].flg & CR_FLG_FILL) != 0
                                         || (blend_info.core[3].flg & CR_FLG_FILL) != 0 {
                                    0.5
                                } else {
                                    blend_info.line_weight
                                };
                                blend_info.core[2].end = blend_info.core[2].start - (blend_info.core[2].start - blend_info.core[2].end) * weight;
                                blend_info.core[3].end = blend_info.core[3].start + (blend_info.core[3].end   - blend_info.core[3].start) * weight;

                                if blend_info.core[0].length >= 2 && blend_info.core[3].length >= 2 {
                                    lack_mode_02_execute(&mut blend_info);
                                } else if blend_info.core[1].length > 0 {
                                    blend_info.mode = BLEND_MODE_UP_H;
                                    up_mode_left_blending(&mut blend_info);
                                    up_mode_right_blending(&mut blend_info);
                                    if blend_info.core[2].length > 1 {
                                        up_mode_top_blending(&mut blend_info);
                                        up_mode_bottom_blending(&mut blend_info);
                                    }
                                } else if blend_info.core[2].length > 0 {
                                    blend_info.mode = BLEND_MODE_UP_V;
                                    up_mode_top_blending(&mut blend_info);
                                    up_mode_bottom_blending(&mut blend_info);
                                }
                            }
                        }

                        5 if (skip & 0x02) == 0 => {
                            // 下向きの角
                            if compare_pixel_equal(&blend_info, in_target + in_width,     in_target + 1)
                                && compare_pixel(&blend_info, in_target + in_width + 1, in_target + in_width)
                                && compare_pixel(&blend_info, in_target + in_width + 1, in_target + 1)
                            {
                                // skip
                            } else {
                                down_mode_left_count_length(&mut blend_info);
                                down_mode_right_count_length(&mut blend_info);
                                down_mode_top_count_length(&mut blend_info);
                                down_mode_bottom_count_length(&mut blend_info);

                                if blend_info.core[0].length - blend_info.core[1].length == 1 {
                                    blend_info.core[0].start -= 0.5;
                                    blend_info.core[1].start -= 0.5;
                                }
                                weight = if (blend_info.core[0].flg & CR_FLG_FILL) != 0
                                         || (blend_info.core[1].flg & CR_FLG_FILL) != 0 {
                                    0.5
                                } else {
                                    blend_info.line_weight
                                };
                                blend_info.core[0].end = blend_info.core[0].start - (blend_info.core[0].start - blend_info.core[0].end) * weight;
                                blend_info.core[1].end = blend_info.core[1].start + (blend_info.core[1].end   - blend_info.core[1].start) * weight;

                                if blend_info.core[3].length - blend_info.core[2].length == 1 {
                                    blend_info.core[2].start += 0.5;
                                    blend_info.core[3].start += 0.5;
                                }
                                weight = if (blend_info.core[2].flg & CR_FLG_FILL) != 0
                                         || (blend_info.core[3].flg & CR_FLG_FILL) != 0 {
                                    0.5
                                } else {
                                    blend_info.line_weight
                                };
                                blend_info.core[2].end = blend_info.core[2].start - (blend_info.core[2].start - blend_info.core[2].end) * weight;
                                blend_info.core[3].end = blend_info.core[3].start + (blend_info.core[3].end   - blend_info.core[3].start) * weight;

                                if blend_info.core[0].length >= 2 && blend_info.core[2].length >= 2 {
                                    lack_mode_01_execute(&mut blend_info);
                                } else if blend_info.core[1].length > 0 {
                                    blend_info.mode = BLEND_MODE_UP_H;
                                    down_mode_left_blending(&mut blend_info);
                                    down_mode_right_blending(&mut blend_info);
                                    if blend_info.core[3].length > 1 {
                                        down_mode_top_blending(&mut blend_info);
                                        down_mode_bottom_blending(&mut blend_info);
                                    }
                                } else if blend_info.core[3].length > 0 {
                                    blend_info.mode = BLEND_MODE_UP_V;
                                    down_mode_top_blending(&mut blend_info);
                                    down_mode_bottom_blending(&mut blend_info);
                                }
                            }
                        }

                        7  if (skip & 0x04) == 0 => link8_mode_01_execute(&mut blend_info),
                        11 if (skip & 0x08) == 0 => link8_mode_02_execute(&mut blend_info),
                        13 if (skip & 0x10) == 0 => link8_mode_04_execute(&mut blend_info),
                        15 if (skip & 0x20) == 0 => link8_square_execute(&mut blend_info),
                        _ => {}
                    }

                    // 突起mode3
                    if i < logical_width - 2 {
                        blend_info.i          = i + 1;
                        blend_info.j          = j;
                        blend_info.in_target  = in_target + 1;
                        blend_info.out_target = out_target + 1;
                        blend_info.flag       = 0;

                        let mut mode_flg2: u8 = 0;
                        if compare_pixel(&blend_info, blend_info.in_target, blend_info.in_target - in_width) { mode_flg2 |= 1 << 0; }
                        if compare_pixel(&blend_info, blend_info.in_target, blend_info.in_target + in_width) { mode_flg2 |= 1 << 1; }
                        if compare_pixel(&blend_info, blend_info.in_target, blend_info.in_target + 1)        { mode_flg2 |= 1 << 2; }

                        if mode_flg2 == 3 && (skip & 0x40) == 0 {
                            link8_mode_03_execute(&mut blend_info);
                        }
                    }
                }
            }

            i += 1;
            in_target  += 1;
            out_target += 1;
        }
    }
}
