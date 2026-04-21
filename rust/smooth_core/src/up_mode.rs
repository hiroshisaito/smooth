// upMode.cpp port.

use crate::blend::blending_f;
use crate::compare::{compare_pixel, compare_pixel_equal};
use crate::types::{BlendingInfo, SmoothPixel, CR_FLG_FILL, SECOND_COUNT};

pub unsafe fn up_mode_left_count_length<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let mut len: i32 = 1;
    let width  = info.width  as i64;
    let w32    = info.width;
    let h32    = info.height;

    loop {
        let count_target = info.in_target - (len as i64 - 1);

        // べた塗り系か？
        if compare_pixel(info, count_target, count_target - 1) {
            info.core[0].start = (info.i + 1) as f32;
            info.core[0].end   = (info.i + 1) as f32 - len as f32;
            info.core[0].flg  |= CR_FLG_FILL;
            break;
        }

        let count_target2 = info.in_target - width - (len as i64 - 1);

        if compare_pixel(info, count_target2, count_target2 - 1) {
            info.core[0].start = (info.i + 1) as f32;
            info.core[0].end   = (info.i + 1) as f32 - len as f32;

            if w32 - 2 > info.i && info.i > 2 && h32 - 2 > info.j && info.j > 2 {
                if (info.flag & SECOND_COUNT as i32) == 0
                    && compare_pixel(info, count_target2 - 1, count_target2 - 1 - width)
                {
                    let mut sc_info: BlendingInfo<P> = info.clone();
                    sc_info.i          = info.i - len;
                    sc_info.j          = info.j - 1;
                    sc_info.in_target  = sc_info.j as i64 * width + sc_info.i as i64;
                    sc_info.out_target = sc_info.in_target;
                    sc_info.flag       = SECOND_COUNT as i32;

                    up_mode_left_count_length(&mut sc_info);

                    if sc_info.core[0].length as i32 - len == 1 {
                        info.core[0].end -= 0.5;
                    }
                }
            }
            break;
        }

        len += 1;

        if info.i - len <= 1 {
            len = info.i - 1;
            info.core[0].start = (info.i + 1) as f32;
            info.core[0].end   = (info.i + 1) as f32 - len as f32;
            break;
        }
    }

    info.core[0].length = len as i64;
}

pub unsafe fn up_mode_right_count_length<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let mut len: i32 = 0;
    let width  = info.width as i64;
    let w32    = info.width;
    let h32    = info.height;

    // 始めの1回は左だけ検査
    let count_target = info.in_target + width;

    if compare_pixel(info, count_target, count_target + 1)
        && compare_pixel_equal(info, info.in_target + 1, info.in_target + 1 + width)
    {
        info.core[1].length = 0;
        return;
    } else {
        len += 1;
        if (info.i + 1) + len >= (w32 - 1) {
            len = w32 - 1 - (info.i + 1);
            info.core[1].start  = (info.i + 1) as f32;
            info.core[1].end    = (info.i + 1) as f32 + len as f32;
            info.core[1].length = len as i64;
            return;
        }
    }

    loop {
        let count_target_f = info.in_target + len as i64;
        if compare_pixel(info, count_target_f, count_target_f + 1) {
            info.core[1].start = (info.i + 1) as f32;
            info.core[1].end   = (info.i + 1 + len) as f32;
            info.core[1].flg  |= CR_FLG_FILL;
            break;
        }

        let count_target2 = info.in_target + width + len as i64;
        if compare_pixel(info, count_target2, count_target2 + 1) {
            info.core[1].start = (info.i + 1) as f32;
            info.core[1].end   = (info.i + 1 + len) as f32;

            if w32 - 2 > info.i && info.i > 2 && h32 - 2 > info.j && info.j > 2 {
                if (info.flag & SECOND_COUNT as i32) == 0
                    && compare_pixel(info, count_target2, count_target2 + 1)
                {
                    let mut sc_info: BlendingInfo<P> = info.clone();
                    sc_info.i          = info.i + len;
                    sc_info.j          = info.j + 1;
                    sc_info.in_target  = sc_info.j as i64 * width + sc_info.i as i64;
                    sc_info.out_target = sc_info.in_target;
                    sc_info.flag       = SECOND_COUNT as i32;

                    up_mode_right_count_length(&mut sc_info);

                    if (len as i64) - sc_info.core[1].length == 1 && sc_info.core[1].length != 0 {
                        info.core[1].end -= 0.5;
                    }
                }
            }
            break;
        }

        len += 1;

        if (info.i + 1) + len >= (w32 - 1) {
            len = w32 - 1 - (info.i + 1);
            info.core[1].start = (info.i + 1) as f32;
            info.core[1].end   = (info.i + 1) as f32 + len as f32;
            break;
        }
    }

    info.core[1].length = len as i64;
}

pub unsafe fn up_mode_top_count_length<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let mut len: i32 = 0;
    let width  = info.width as i64;
    let w32    = info.width;
    let h32    = info.height;

    // 始めの1回は左だけ検査
    let count_target = info.in_target - 1;
    if compare_pixel(info, count_target, count_target - width)
        && compare_pixel_equal(info, info.in_target - width, info.in_target - 1 - width)
    {
        info.core[2].length = 0;
        info.core[2].start  = info.j as f32;
        info.core[2].end    = info.core[2].start;
        return;
    } else {
        len += 1;
        if info.j - len <= 1 {
            len = info.j - 1;
            info.core[2].start  = info.j as f32;
            info.core[2].end    = (info.j - len) as f32;
            info.core[2].length = len as i64;
            return;
        }
    }

    loop {
        let count_target = info.in_target - (len as i64) * width;
        if compare_pixel(info, count_target, count_target - width) {
            info.core[2].start = info.j as f32;
            info.core[2].end   = (info.j - len) as f32;
            info.core[2].flg  |= CR_FLG_FILL;
            break;
        }

        let count_target2 = info.in_target - (len as i64) * width - 1;
        if compare_pixel(info, count_target2, count_target2 - width) {
            info.core[2].start = info.j as f32;
            info.core[2].end   = (info.j - len) as f32;

            if w32 - 2 > info.i && info.i > 2 && h32 - 2 > info.j && info.j > 2 {
                if (info.flag & SECOND_COUNT as i32) == 0
                    && compare_pixel(info, count_target2, count_target2 + 1)
                {
                    let mut sc_info: BlendingInfo<P> = info.clone();
                    sc_info.i          = info.i - 1;
                    sc_info.j          = info.j - len;
                    sc_info.in_target  = sc_info.j as i64 * width + sc_info.i as i64;
                    sc_info.out_target = sc_info.in_target;
                    sc_info.flag       = SECOND_COUNT as i32;

                    up_mode_top_count_length(&mut sc_info);

                    if (len as i64) - sc_info.core[2].length == 1 && sc_info.core[2].length != 0 {
                        info.core[2].end += 0.5;
                    }
                }
            }
            break;
        }

        len += 1;

        if info.j - len <= 1 {
            len = info.j - 1;
            info.core[2].start = info.j as f32;
            info.core[2].end   = (info.j - len) as f32;
            break;
        }
    }

    info.core[2].length = len as i64;
}

pub unsafe fn up_mode_bottom_count_length<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let mut len: i32 = 1;
    let width  = info.width as i64;
    let w32    = info.width;
    let h32    = info.height;

    loop {
        let count_target = info.in_target + (len as i64 - 1) * width;
        if compare_pixel(info, count_target, count_target + width) {
            info.core[3].start = info.j as f32;
            info.core[3].end   = (info.j + len) as f32;
            info.core[3].flg  |= CR_FLG_FILL;
            break;
        }

        let count_target2 = info.in_target + (len as i64 - 1) * width + 1;
        if compare_pixel(info, count_target2, count_target2 + width) {
            info.core[3].start = info.j as f32;
            info.core[3].end   = (info.j + len) as f32;

            if w32 - 2 > info.i && info.i > 2 && h32 - 2 > info.j && info.j > 2 {
                if (info.flag & SECOND_COUNT as i32) == 0
                    && compare_pixel(info, count_target2 + width, count_target2 + width + 1)
                {
                    let mut sc_info: BlendingInfo<P> = info.clone();
                    sc_info.i          = info.i + 1;
                    sc_info.j          = info.j + len;
                    sc_info.in_target  = sc_info.j as i64 * width + sc_info.i as i64;
                    sc_info.out_target = sc_info.in_target;
                    sc_info.flag       = SECOND_COUNT as i32;

                    up_mode_bottom_count_length(&mut sc_info);

                    if sc_info.core[3].length - len as i64 == 1 {
                        info.core[3].end += 0.5;
                    }
                }
            }
            break;
        }

        len += 1;

        if info.j + len >= h32 - 1 {
            len = h32 - 1 - info.j;
            info.core[3].start = info.j as f32;
            info.core[3].end   = (info.j + len) as f32;
            break;
        }
    }

    info.core[3].length = len as i64;
}

pub unsafe fn up_mode_left_blending<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let in_width = info.width as i64;
    let end   = info.core[0].end;

    let end_p = end as i32;
    let len = info.core[0].start - end;

    let blend_count = ((info.i + 1) as f32 - end).ceil() as i32;
    let mut pre_ratio: f32 = 0.0;
    let mut blend_target = info.in_target  - (blend_count - 1) as i64;
    let mut out_target   = info.out_target - (blend_count - 1) as i64;

    for t in 0..blend_count {
        let l: f32 = (end_p + 1 + t) as f32 - end;
        let ratio: f32 = (l * l * 0.5 * 0.5) / len;
        blending_f(info.in_ptr, info.out_ptr, blend_target, blend_target - in_width, out_target, 1.0 - (ratio - pre_ratio));
        pre_ratio = ratio;
        blend_target += 1;
        out_target   += 1;
    }
}

pub unsafe fn up_mode_right_blending<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let length = info.core[1].length;
    let in_width = info.width as i64;
    let start = info.core[1].start;
    let end   = info.core[1].end;

    if length <= 0 { return; }

    let end_p = (end as f64 - 0.000001) as i32;
    let len = end - start;

    let blend_count = (end - (info.i + 1) as f32).ceil() as i32;
    let mut pre_ratio: f32 = 0.0;
    let mut blend_target = info.in_target  + blend_count as i64;
    let mut out_target   = info.out_target + blend_count as i64;

    for t in 0..blend_count {
        let l: f32 = end - (end_p - t) as f32;
        let ratio: f32 = (l * l * 0.5 * 0.5) / len;
        blending_f(info.in_ptr, info.out_ptr, blend_target, blend_target + in_width, out_target, 1.0 - (ratio - pre_ratio));
        pre_ratio = ratio;
        blend_target -= 1;
        out_target   -= 1;
    }
}

pub unsafe fn up_mode_top_blending<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let length = info.core[2].length;
    let in_width  = info.width as i64;
    let out_width = info.width as i64;
    let start = info.core[2].start;
    let end   = info.core[2].end;

    if length <= 0 { return; }

    let end_p = end as i32;
    let len = start - end;
    let blend_count = (info.j as f32 - end).ceil() as i32;

    let mut pre_ratio: f32 = 0.0;
    let mut blend_target = info.in_target  - blend_count as i64 * in_width;
    let mut out_target   = info.out_target - blend_count as i64 * out_width;

    for t in 0..blend_count {
        let l: f32 = (end_p + 1 + t) as f32 - end;
        let ratio: f32 = (l * l * 0.5 * 0.5) / len;
        blending_f(info.in_ptr, info.out_ptr, blend_target, blend_target - 1, out_target, 1.0 - (ratio - pre_ratio));
        pre_ratio = ratio;
        blend_target += in_width;
        out_target   += out_width;
    }
}

pub unsafe fn up_mode_bottom_blending<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let in_width  = info.width as i64;
    let out_width = info.width as i64;
    let start = info.core[3].start;
    let end   = info.core[3].end;

    let end_p = (end as f64 - 0.00001) as i32;
    let len = end - start;
    let blend_count = (end - info.j as f32).ceil() as i32;

    let mut pre_ratio: f32 = 0.0;
    let mut blend_target = info.in_target  + (blend_count - 1) as i64 * in_width;
    let mut out_target   = info.out_target + (blend_count - 1) as i64 * out_width;

    for t in 0..blend_count {
        let l: f32 = end - (end_p - t) as f32;
        let ratio: f32 = (l * l * 0.5 * 0.5) / len;
        blending_f(info.in_ptr, info.out_ptr, blend_target, blend_target + 1, out_target, 1.0 - (ratio - pre_ratio));
        pre_ratio = ratio;
        blend_target -= in_width;
        out_target   -= out_width;
    }
}
