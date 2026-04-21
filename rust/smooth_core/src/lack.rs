// Lack.cpp port.

use crate::blend::blending_pixel_f;
use crate::compare::{compare_pixel, compare_pixel_equal};
use crate::types::{BlendingInfo, SmoothPixel, px_read, px_write};

/// Lack mode 01
pub unsafe fn lack_mode_01_execute<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let width = info.width as i64;
    let h = info.core[0].length;
    let v = info.core[2].length;

    if compare_pixel(info, info.in_target, info.in_target - width - 1) {
        return;
    }

    if (3 >= h && h >= 2) && (3 >= v && v >= 2) {
        let in_ptr = info.in_ptr;
        let src  = px_read(in_ptr, info.in_target);
        let ref0 = px_read(in_ptr, info.in_target + 1);
        let ref1 = px_read(in_ptr, info.in_target + width);
        let ref2 = px_read(in_ptr, info.in_target + width + 1);

        let mut ref_temp = src; // any P, fields overwritten below
        ref_temp.set_red(  (ref0.red()   + ref1.red()   + ref2.red())   / 3);
        ref_temp.set_green((ref0.green() + ref1.green() + ref2.green()) / 3);
        ref_temp.set_blue( (ref0.blue()  + ref1.blue()  + ref2.blue())  / 3);
        ref_temp.set_alpha((ref0.alpha() + ref1.alpha() + ref2.alpha()) / 3);

        let mut out_px = px_read(info.out_ptr, info.out_target);
        blending_pixel_f(&src, &ref_temp, &mut out_px, 0.5);
        px_write(info.out_ptr, info.out_target, out_px);
    }
}

/// Lack mode 02
pub unsafe fn lack_mode_02_execute<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let width = info.width as i64;
    let h = info.core[0].length;
    let v = info.core[3].length;

    if compare_pixel(info, info.in_target, info.in_target + width - 1) {
        return;
    }

    if (3 >= h && h >= 2) && (3 >= v && v >= 2) {
        let in_ptr = info.in_ptr;
        let src  = px_read(in_ptr, info.in_target);
        let ref0 = px_read(in_ptr, info.in_target + 1);
        let ref1 = px_read(in_ptr, info.in_target - width);
        let ref2 = px_read(in_ptr, info.in_target - width + 1);

        let mut ref_temp = src;
        ref_temp.set_red(  (ref0.red()   + ref1.red()   + ref2.red())   / 3);
        ref_temp.set_green((ref0.green() + ref1.green() + ref2.green()) / 3);
        ref_temp.set_blue( (ref0.blue()  + ref1.blue()  + ref2.blue())  / 3);
        ref_temp.set_alpha((ref0.alpha() + ref1.alpha() + ref2.alpha()) / 3);

        let mut out_px = px_read(info.out_ptr, info.out_target);
        blending_pixel_f(&src, &ref_temp, &mut out_px, 0.5);
        px_write(info.out_ptr, info.out_target, out_px);
    }
}

/// Lack mode 03/04
pub unsafe fn lack_mode_0304_execute<P: SmoothPixel>(info: &mut BlendingInfo<P>) {
    let width = info.width as i64;

    let mut h: i64 = 1;
    let mut v: i64 = 1;
    let mut target = info.in_target;

    // mode 03?
    if compare_pixel_equal(info, target, target - width + 1)
        && compare_pixel(info, target, target + width)
    {
        // →
        let mut i = info.i;
        while i < info.logical_width - 1 {
            if compare_pixel(info, target, target + 1)
                || compare_pixel(info, target + width, target + width + 1)
            {
                break;
            }
            h += 1;
            i += 1;
            target += 1;
        }

        // ↑
        target = info.in_target;
        let mut j = info.j;
        while j > 1 {
            if compare_pixel(info, target, target - width)
                || compare_pixel(info, target - 1, target - 1 - width)
            {
                break;
            }
            v += 1;
            j -= 1;
            target -= width;
        }

        if (3 >= h && h >= 2) && (3 >= v && v >= 2) {
            let in_ptr = info.in_ptr;
            let src  = px_read(in_ptr, info.in_target);
            let ref0 = px_read(in_ptr, info.in_target - 1);
            let ref1 = px_read(in_ptr, info.in_target + width);
            let ref2 = px_read(in_ptr, info.in_target + width - 1);

            let mut ref_temp = src;
            ref_temp.set_red(  (ref0.red()   + ref1.red()   + ref2.red())   / 3);
            ref_temp.set_green((ref0.green() + ref1.green() + ref2.green()) / 3);
            ref_temp.set_blue( (ref0.blue()  + ref1.blue()  + ref2.blue())  / 3);
            ref_temp.set_alpha((ref0.alpha() + ref1.alpha() + ref2.alpha()) / 3);

            let mut out_px = px_read(info.out_ptr, info.out_target);
            blending_pixel_f(&src, &ref_temp, &mut out_px, 0.5);
            px_write(info.out_ptr, info.out_target, out_px);
        }
    }
    // mode 04?
    else if compare_pixel_equal(info, target, target + width + 1)
        && compare_pixel(info, target, target - width)
    {
        // →
        let mut i = info.i;
        while i < info.logical_width - 1 {
            if compare_pixel(info, target, target + 1)
                || compare_pixel(info, target - width, target - width + 1)
            {
                break;
            }
            h += 1;
            i += 1;
            target += 1;
        }

        // ↓
        target = info.in_target;
        let mut j = info.j;
        while j < info.height - 1 {
            if compare_pixel(info, target, target + width)
                || compare_pixel(info, target - 1, target - 1 + width)
            {
                break;
            }
            v += 1;
            j += 1;
            target += width;
        }

        if (3 >= h && h >= 2) && (3 >= v && v >= 2) {
            let in_ptr = info.in_ptr;
            let src  = px_read(in_ptr, info.in_target);
            let ref0 = px_read(in_ptr, info.in_target - 1);
            let ref1 = px_read(in_ptr, info.in_target - width);
            let ref2 = px_read(in_ptr, info.in_target - width - 1);

            let mut ref_temp = src;
            ref_temp.set_red(  (ref0.red()   + ref1.red()   + ref2.red())   / 3);
            ref_temp.set_green((ref0.green() + ref1.green() + ref2.green()) / 3);
            ref_temp.set_blue( (ref0.blue()  + ref1.blue()  + ref2.blue())  / 3);
            ref_temp.set_alpha((ref0.alpha() + ref1.alpha() + ref2.alpha()) / 3);

            let mut out_px = px_read(info.out_ptr, info.out_target);
            blending_pixel_f(&src, &ref_temp, &mut out_px, 0.5);
            px_write(info.out_ptr, info.out_target, out_px);
        }
    }
}
