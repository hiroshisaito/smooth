// 8link.cpp port.

use crate::blend::{blending_pixel_f, blend_line};
use crate::compare::{compare_pixel, compare_pixel_equal};
use crate::types::{BlendingInfo, SmoothPixel, SmoothScalar, px_read, px_write};

const MAX_LENGTH: usize = 128;

#[inline]
fn get_sign(a: i32) -> i32 {
    if a > 0 { 1 } else if a < 0 { -1 } else { 0 }
}

unsafe fn count_length<P: SmoothPixel>(
    info: &BlendingInfo<P>,
    target: i64,
    next_pixel_step_in: i32,
    min: i32, max: i32, limit_from_here: i32,
) -> i32 {
    let mut length: i32 = 0;
    let sign = get_sign(next_pixel_step_in);
    let len_diff = sign * 1;

    while min < length + limit_from_here && length + limit_from_here < max {
        let t = target + length as i64 * next_pixel_step_in as i64;
        length += len_diff;
        if compare_pixel(info, t, t + next_pixel_step_in as i64) {
            break;
        }
    }

    length.abs()
}

unsafe fn count_length_two_lines<P: SmoothPixel>(
    info: &BlendingInfo<P>,
    target0: i64, target1: i64,
    next_pixel_step_in: i32,
    min: i32, max: i32, limit_from_here: i32,
    t0_flg: &mut bool,
) -> i32 {
    let mut length: i32 = 0;
    let sign = get_sign(next_pixel_step_in);
    let len_diff = sign * 1;

    *t0_flg = false;

    while min < length + limit_from_here && length + limit_from_here < max {
        let t0 = target0 + (length.abs() as i64) * next_pixel_step_in as i64;
        let t1 = target1 + (length.abs() as i64) * next_pixel_step_in as i64;
        length += len_diff;

        if compare_pixel(info, t0, t0 + next_pixel_step_in as i64) {
            *t0_flg = true;
            break;
        }
        if compare_pixel(info, t1, t1 + next_pixel_step_in as i64) {
            break;
        }
    }

    length.abs()
}

unsafe fn blend_outside<P: SmoothPixel>(
    info: &BlendingInfo<P>,
    length: f64,
    mut blend_target: i64, mut out_target: i64,
    ref_offset: i32,
    next_pixel_step_in: i32, next_pixel_step_out: i32,
    ratio_invert: bool, no_line_weight: bool,
) {
    let len: f64 = if no_line_weight {
        length * 0.5
    } else {
        length * info.line_weight as f64
    };

    let blend_count = len.ceil() as i32;

    blend_target += (blend_count - 1) as i64 * next_pixel_step_in  as i64;
    out_target   += (blend_count - 1) as i64 * next_pixel_step_out as i64;

    let mut pre_ratio: f64 = 0.0;
    for t in 0..blend_count {
        let l: f64 = len - ((len.ceil() as i32 - 1 - t) as f32) as f64;
        let ratio: f64 = (l * l * 0.5 * 0.5) / len;
        let r: f64 = if ratio_invert { 1.0 - (ratio - pre_ratio) } else { ratio - pre_ratio };

        crate::blend::blending_f(info.in_ptr, info.out_ptr, blend_target, blend_target + ref_offset as i64, out_target, r as f32);

        pre_ratio = ratio;
        blend_target -= next_pixel_step_in  as i64;
        out_target   -= next_pixel_step_out as i64;
    }
}

unsafe fn blend_inside<P: SmoothPixel>(
    temp_pixel: &mut [[P; MAX_LENGTH]; 2],
    index: usize,
    info: &BlendingInfo<P>,
    length: f64,
    mut blend_target: i64,
    ref_offset: i32,
    next_pixel_step_in: i32,
    ratio_invert: bool, no_line_weight: bool,
) {
    let len: f64 = if no_line_weight {
        length * 0.5
    } else {
        length * info.line_weight as f64
    };

    let blend_count = len.ceil() as i32;

    blend_target += (blend_count - 1) as i64 * next_pixel_step_in as i64;

    let mut pre_ratio: f64 = 0.0;
    for t in 0..blend_count {
        let l: f64 = len - ((len.ceil() as i32 - 1 - t) as f32) as f64;
        let ratio: f64 = (l * l * 1.0 * 0.5) / len;
        let r: f64 = if ratio_invert { 1.0 - (ratio - pre_ratio) } else { ratio - pre_ratio };

        let a = px_read(info.in_ptr, blend_target);
        let b = px_read(info.in_ptr, blend_target + ref_offset as i64);
        let dst = &mut temp_pixel[index][(blend_count - 1 - t) as usize];
        blending_pixel_f(&a, &b, dst, r as f32);

        pre_ratio = ratio;
        blend_target -= next_pixel_step_in as i64;
    }
}

unsafe fn link8_execute<P: SmoothPixel>(
    info: &mut BlendingInfo<P>,
    ref_pixel_step_in: i32,
    ref_pixel_step_out: i32,
    next_pixel_step_in: i32,
    next_pixel_step_out: i32,
    min: i32, max: i32, limit_from_here: i32,
    area_min: i32, area_max: i32, area_position: i32,
    mode: i32,
) {
    let in_target  = info.in_target;
    let out_target = info.out_target;

    let mut temp_pixel: [[P; MAX_LENGTH]; 2] = [[P::null_pixel(); MAX_LENGTH]; 2];
    let mut length: [i32; 2] = [0, 0];
    let mut inside_flg: [bool; 2] = [false, false];
    let mut flag = false;

    length[0] = count_length_two_lines(info, in_target, in_target - ref_pixel_step_in as i64, next_pixel_step_in,
                                       min, max, limit_from_here, &mut flag);
    length[1] = count_length_two_lines(info, in_target, in_target + ref_pixel_step_in as i64, next_pixel_step_in,
                                       min, max, limit_from_here, &mut flag);

    length[0] = length[0].min(MAX_LENGTH as i32);
    length[1] = length[1].min(MAX_LENGTH as i32);

    let temp_length = length[0].max(length[1]);

    let mut force_inside_flag = false;
    if area_min < area_position && area_position < area_max {
        if compare_pixel(info, in_target - ref_pixel_step_in as i64, in_target - ref_pixel_step_in as i64 * 2)
            && compare_pixel(info, in_target + ref_pixel_step_in as i64, in_target + ref_pixel_step_in as i64 * 2)
        {
            force_inside_flag = true;
        }
    }

    // ----- Left/Top side -----
    if compare_pixel_equal(info, in_target, in_target - next_pixel_step_in as i64 - ref_pixel_step_in as i64) && !force_inside_flag {
        let mut flg = false;
        if area_min < area_position && area_position < area_max {
            if compare_pixel_equal(info, in_target - ref_pixel_step_in as i64, in_target - ref_pixel_step_in as i64 * 2) {
                flg = true;
            }
        } else {
            flg = true;
        }
        if flg {
            blend_outside(info,
                          length[0] as f64,
                          info.in_target  - ref_pixel_step_in  as i64,
                          info.out_target - ref_pixel_step_out as i64,
                          ref_pixel_step_in,
                          next_pixel_step_in, next_pixel_step_out,
                          true, true);
        }
    } else {
        blend_inside(&mut temp_pixel, 0, info,
                     length[0] as f64,
                     info.in_target,
                     -ref_pixel_step_in,
                     next_pixel_step_in,
                     true, true);
        inside_flg[0] = true;
    }

    // ----- Right/Bottom side -----
    if compare_pixel_equal(info, in_target, in_target - next_pixel_step_in as i64 + ref_pixel_step_in as i64) && !force_inside_flag {
        let mut flg = false;
        if area_min < area_position && area_position < area_max {
            if compare_pixel_equal(info, in_target + ref_pixel_step_in as i64, in_target + ref_pixel_step_in as i64 * 2) {
                flg = true;
            }
        } else {
            flg = true;
        }
        if flg {
            blend_outside(info,
                          length[1] as f64,
                          info.in_target  + ref_pixel_step_in  as i64,
                          info.out_target + ref_pixel_step_out as i64,
                          -ref_pixel_step_in,
                          next_pixel_step_in, next_pixel_step_out,
                          true, true);
        }
    } else {
        blend_inside(&mut temp_pixel, 1, info,
                     length[1] as f64,
                     info.in_target,
                     ref_pixel_step_in,
                     next_pixel_step_in,
                     true, true);
        inside_flg[1] = true;
    }

    // Both inside + different colors on two sides
    if compare_pixel(info, in_target - ref_pixel_step_in as i64, in_target + ref_pixel_step_in as i64)
        && inside_flg[0] && inside_flg[1]
    {
        let mut blend_flg = false;
        let f0 = compare_pixel(info, info.in_target - next_pixel_step_in as i64, info.in_target - next_pixel_step_in as i64 + ref_pixel_step_in as i64);
        let f1 = compare_pixel(info, info.in_target - next_pixel_step_in as i64, info.in_target - next_pixel_step_in as i64 - ref_pixel_step_in as i64);

        if !f0 && !f1 {
            blend_flg = true;
        }

        match mode {
            2 | 4 => {
                if (mode == 2 && !f0 && f1) || (mode == 4 && !f0 && f1) {
                    blend_flg = true;
                }
            }
            1 => { blend_flg = true; }
            _ => {}
        }

        if blend_flg {
            let len = length[0].min(length[1]) as f64;

            if compare_pixel_equal(info, info.in_target - next_pixel_step_in as i64, info.in_target + ref_pixel_step_in as i64) {
                blend_line(info, len, in_target, out_target,
                           ref_pixel_step_in, next_pixel_step_in, next_pixel_step_out,
                           true, true);
            } else if compare_pixel_equal(info, info.in_target - next_pixel_step_in as i64, info.in_target - ref_pixel_step_in as i64) {
                blend_line(info, len, in_target, out_target,
                           -ref_pixel_step_in, next_pixel_step_in, next_pixel_step_out,
                           true, true);
            }
        }
    } else {
        // Average the two inside outputs
        let total_len = ((temp_length as f32) * 0.5).ceil() as i32;
        let len0 = ((length[0] as f32) * 0.5).ceil() as i32;
        let len1 = ((length[1] as f32) * 0.5).ceil() as i32;

        for i in 0..total_len {
            let out_off = out_target + i as i64 * next_pixel_step_out as i64;
            if (i < len0 && inside_flg[0]) && (i < len1 && inside_flg[1]) {
                let p0 = temp_pixel[0][i as usize];
                let p1 = temp_pixel[1][i as usize];
                let mut out = px_read(info.out_ptr, out_off);
                out.set_red(  (p0.red()   + p1.red()  ).div_by_int(2));
                out.set_green((p0.green() + p1.green()).div_by_int(2));
                out.set_blue( (p0.blue()  + p1.blue() ).div_by_int(2));
                out.set_alpha((p0.alpha() + p1.alpha()).div_by_int(2));
                px_write(info.out_ptr, out_off, out);
            } else if i < len0 && inside_flg[0] {
                let a = px_read(info.in_ptr, in_target + i as i64 * next_pixel_step_in as i64);
                let b = temp_pixel[0][i as usize];
                let mut out = px_read(info.out_ptr, out_off);
                blending_pixel_f(&a, &b, &mut out, 0.5);
                px_write(info.out_ptr, out_off, out);
            } else if i < len1 && inside_flg[1] {
                let a = px_read(info.in_ptr, in_target + i as i64 * next_pixel_step_in as i64);
                let b = temp_pixel[1][i as usize];
                let mut out = px_read(info.out_ptr, out_off);
                blending_pixel_f(&a, &b, &mut out, 0.5);
                px_write(info.out_ptr, out_off, out);
            }
        }
    }

    // Pattern not handled by up/down mode
    if compare_pixel_equal(info, in_target, in_target + next_pixel_step_in as i64 - ref_pixel_step_in as i64)
        && compare_pixel_equal(info, in_target, in_target + next_pixel_step_in as i64 + ref_pixel_step_in as i64)
    {
        match mode {
            1 => {
                let mut len = [0i32; 2];
                len[0] = count_length_two_lines(info,
                                                in_target - ref_pixel_step_in as i64,
                                                in_target - ref_pixel_step_in as i64 + next_pixel_step_in as i64,
                                                -ref_pixel_step_in,
                                                area_min, area_max, area_position,
                                                &mut flag);
                blend_line(info, len[0] as f64,
                           in_target  - ref_pixel_step_in  as i64,
                           out_target - ref_pixel_step_out as i64,
                           -ref_pixel_step_in + next_pixel_step_in,
                           -ref_pixel_step_in, -ref_pixel_step_out,
                           true, true);

                len[1] = count_length_two_lines(info,
                                                in_target + ref_pixel_step_in as i64,
                                                in_target + ref_pixel_step_in as i64 + next_pixel_step_in as i64,
                                                ref_pixel_step_in,
                                                area_min, area_max, area_position,
                                                &mut flag);
                blend_line(info, len[1] as f64,
                           in_target  + ref_pixel_step_in  as i64,
                           out_target + ref_pixel_step_out as i64,
                           ref_pixel_step_in + next_pixel_step_in,
                           ref_pixel_step_in, ref_pixel_step_out,
                           true, true);
            }
            2 | 4 => {
                // カウント (右)
                let len2 = count_length_two_lines(info,
                                                  in_target + ref_pixel_step_in as i64,
                                                  in_target + ref_pixel_step_in as i64 + next_pixel_step_in as i64,
                                                  ref_pixel_step_in,
                                                  area_min, area_max, area_position,
                                                  &mut flag);
                blend_line(info, len2 as f64,
                           in_target  + ref_pixel_step_in  as i64,
                           out_target + ref_pixel_step_out as i64,
                           ref_pixel_step_in + next_pixel_step_in,
                           ref_pixel_step_in, ref_pixel_step_out,
                           true, true);
            }
            _ => {} // mode 3: do nothing
        }
    }
}

pub unsafe fn link8_mode_01_execute<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let in_width  = info.width;
    let in_height = info.height;
    let out_width = info.width;
    link8_execute(info, -in_width, -out_width, -1, -1,
                  0, in_width - 1, info.i,
                  1, in_height - 2, info.j,
                  1);
}

pub unsafe fn link8_mode_02_execute<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let in_width  = info.width;
    let in_height = info.height;
    let out_width = info.width;
    link8_execute(info, 1, 1, in_width, out_width,
                  0, in_height - 1, info.j,
                  1, in_width - 2, info.i,
                  2);
}

pub unsafe fn link8_mode_03_execute<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let in_width  = info.width;
    let in_height = info.height;
    let out_width = info.width;
    link8_execute(info, -in_width, -out_width, 1, 1,
                  0, in_width - 1, info.i,
                  1, in_height - 2, info.j,
                  3);
}

pub unsafe fn link8_mode_04_execute<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let in_width  = info.width;
    let in_height = info.height;
    let out_width = info.width;
    link8_execute(info, 1, 1, -in_width, -out_width,
                  0, in_height - 1, info.j,
                  1, in_width - 2, info.i,
                  4);
}

unsafe fn link8_square_blend_outside<P: SmoothPixel>(
    info: &BlendingInfo<P>,
    in_target: i64, out_target: i64,
    ref_offset: i32,
    next_pixel_step_in: i32, next_pixel_step_out: i32,
    min: i32, max: i32, limit_from_here: i32,
) {
    let mut no_line_weight = false;
    let count = count_length_two_lines(info, in_target, in_target + ref_offset as i64, next_pixel_step_in,
                                       min, max, limit_from_here, &mut no_line_weight);

    blend_line(info, count as f64,
               in_target, out_target,
               ref_offset, next_pixel_step_in, next_pixel_step_out,
               true, no_line_weight);
}

pub unsafe fn link8_square_execute<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let in_width  = info.width;
    let out_width = info.width;
    let in_height = info.height;
    let iw64 = in_width as i64;

    let in_target  = info.in_target;
    let out_target = info.out_target;

    let mut flg: u32 = 0;
    if compare_pixel_equal(info, in_target, in_target - iw64 - 1) { flg |= 1 << 0; }
    if compare_pixel_equal(info, in_target, in_target - iw64 + 1) { flg |= 1 << 1; }
    if compare_pixel_equal(info, in_target, in_target + iw64 + 1) { flg |= 1 << 2; }
    if compare_pixel_equal(info, in_target, in_target + iw64 - 1) { flg |= 1 << 3; }

    {
        let mut temp_pixel: [P; 4] = [px_read(info.in_ptr, in_target); 4];
        let ref_tbl: [i64; 4] = [-iw64 - 1, -iw64 + 1, iw64 + 1, iw64 - 1];
        let mut sum_color: [P::Scalar; 4] = [<P::Scalar as SmoothScalar>::zero(); 4];

        for i in 0..4 {
            temp_pixel[i] = px_read(info.in_ptr, in_target);
            if (flg & (1u32 << i)) == 0 {
                let a = px_read(info.in_ptr, in_target);
                let b = px_read(info.in_ptr, in_target + ref_tbl[i]);
                blending_pixel_f(&a, &b, &mut temp_pixel[i], 0.5);
            }
            sum_color[0] += temp_pixel[i].red();
            sum_color[1] += temp_pixel[i].green();
            sum_color[2] += temp_pixel[i].blue();
            sum_color[3] += temp_pixel[i].alpha();
        }

        let mut out = px_read(info.out_ptr, out_target);
        out.set_red(  sum_color[0].div_by_int(4));
        out.set_green(sum_color[1].div_by_int(4));
        out.set_blue( sum_color[2].div_by_int(4));
        out.set_alpha(sum_color[3].div_by_int(4));
        px_write(info.out_ptr, out_target, out);
    }

    if (flg & 0x9) != 0x9 {
        if (flg & (1 << 0)) != 0 {
            link8_square_blend_outside(info,
                                       info.in_target - 1, info.out_target - 1,
                                       -in_width, -1, -1,
                                       1, in_width - 2, info.i);
        } else if (flg & (1 << 3)) != 0 {
            link8_square_blend_outside(info,
                                       info.in_target - 1, info.out_target - 1,
                                       in_width, -1, -1,
                                       1, in_width - 2, info.i);
        }
    }

    if (flg & 0x3) != 0x3 {
        if (flg & (1 << 0)) != 0 {
            link8_square_blend_outside(info,
                                       info.in_target - iw64, info.out_target - out_width as i64,
                                       -1, -in_width, -out_width,
                                       1, in_height - 2, info.j);
        } else if (flg & (1 << 1)) != 0 {
            link8_square_blend_outside(info,
                                       info.in_target - iw64, info.out_target - out_width as i64,
                                       1, -in_width, -out_width,
                                       1, in_height - 2, info.j);
        }
    }

    if (flg & 0x6) != 0x6 {
        if (flg & (1 << 1)) != 0 {
            link8_square_blend_outside(info,
                                       info.in_target + 1, info.out_target + 1,
                                       -in_width, 1, 1,
                                       1, in_width - 2, info.i);
        } else if (flg & (1 << 2)) != 0 {
            link8_square_blend_outside(info,
                                       info.in_target + 1, info.out_target + 1,
                                       in_width, 1, 1,
                                       1, in_width - 2, info.i);
        }
    }

    if (flg & 0xc) != 0xc {
        if (flg & (1 << 2)) != 0 {
            link8_square_blend_outside(info,
                                       info.in_target + iw64, info.out_target + out_width as i64,
                                       1, in_width, out_width,
                                       1, in_height - 2, info.j);
        } else if (flg & (1 << 3)) != 0 {
            link8_square_blend_outside(info,
                                       info.in_target + iw64, info.out_target + out_width as i64,
                                       -1, in_width, out_width,
                                       1, in_height - 2, info.j);
        }
    }

    // Silence the (potentially unused due to macro-style tt0_flg) warning.
    let _ = count_length::<P>;
}
