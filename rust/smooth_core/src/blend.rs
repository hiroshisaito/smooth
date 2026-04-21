// BlendingPixelf / Blendingf / BlendLine — ported from util.h/util.cpp.

use crate::types::{BlendingInfo, SmoothPixel, px_read, px_write};

/// BlendingPixelf: blend `target` and `ref` by ratio, write to `output`.
/// Matches util.h::BlendingPixelf semantics for AE's ARGB pre-multiplied alpha case split.
#[inline(always)]
pub fn blending_pixel_f<P: SmoothPixel>(target_pixel: &P, ref_pixel: &P, output_pixel: &mut P, ratio: f32) {
    let max_value = P::max_value();
    let alpha   = (max_value as f32 * ratio) as u32;
    let r_alpha = max_value - alpha;

    let tp_alpha = target_pixel.alpha();
    let rp_alpha = ref_pixel.alpha();

    if tp_alpha == max_value && rp_alpha == max_value {
        output_pixel.set_alpha(max_value);
        output_pixel.set_red(  ((target_pixel.red()   * alpha) + (ref_pixel.red()   * r_alpha)) / max_value);
        output_pixel.set_green(((target_pixel.green() * alpha) + (ref_pixel.green() * r_alpha)) / max_value);
        output_pixel.set_blue( ((target_pixel.blue()  * alpha) + (ref_pixel.blue()  * r_alpha)) / max_value);
    } else if tp_alpha == 0 {
        output_pixel.set_alpha(((tp_alpha * alpha) + (rp_alpha * r_alpha)) / max_value);
        output_pixel.set_red(  ref_pixel.red());
        output_pixel.set_green(ref_pixel.green());
        output_pixel.set_blue( ref_pixel.blue());
    } else if rp_alpha == 0 {
        output_pixel.set_alpha(((tp_alpha * alpha) + (rp_alpha * r_alpha)) / max_value);
        output_pixel.set_red(  target_pixel.red());
        output_pixel.set_green(target_pixel.green());
        output_pixel.set_blue( target_pixel.blue());
    } else {
        output_pixel.set_alpha(((tp_alpha * alpha) + (rp_alpha * r_alpha)) / max_value);
        output_pixel.set_red(  ((target_pixel.red()   * alpha) + (ref_pixel.red()   * r_alpha)) / max_value);
        output_pixel.set_green(((target_pixel.green() * alpha) + (ref_pixel.green() * r_alpha)) / max_value);
        output_pixel.set_blue( ((target_pixel.blue()  * alpha) + (ref_pixel.blue()  * r_alpha)) / max_value);
    }
}

/// Blendingf(in_ptr, out_ptr, blend_target, ref_target, out_target, ratio).
#[inline(always)]
pub unsafe fn blending_f<P: SmoothPixel>(
    in_ptr: *mut P, out_ptr: *mut P,
    blend_target: i64, ref_target: i64, out_target: i64,
    ratio: f32,
) {
    let a = px_read(in_ptr, blend_target);
    let b = px_read(in_ptr, ref_target);
    let mut out = px_read(out_ptr, out_target);
    blending_pixel_f(&a, &b, &mut out, ratio);
    px_write(out_ptr, out_target, out);
}

/// BlendLine — ported from util.cpp.
pub unsafe fn blend_line<P: SmoothPixel>(
    pinfo: &BlendingInfo<P>,
    length: f64,
    mut blend_target: i64,
    mut out_target: i64,
    ref_offset: i32,
    next_pixel_step_in: i32,
    next_pixel_step_out: i32,
    ratio_invert: bool,
    no_line_weight: bool,
) {
    let len: f64 = if no_line_weight {
        length * 0.5
    } else {
        length * pinfo.line_weight as f64
    };

    let blend_count = len.ceil() as i32;

    blend_target += (blend_count - 1) as i64 * next_pixel_step_in  as i64;
    out_target   += (blend_count - 1) as i64 * next_pixel_step_out as i64;

    let mut pre_ratio: f64 = 0.0;
    for t in 0..blend_count {
        // ((int)CEIL(len)-1 は (1.000...1 ～ 2.0) -> 1 としたい) → integer ceil - 1
        let l: f64 = len - ((len.ceil() as i32 - 1 - t) as f32) as f64;
        let ratio: f64 = (l * l * 0.5 * 0.5) / len;
        let r: f64 = if ratio_invert { 1.0 - (ratio - pre_ratio) } else { ratio - pre_ratio };

        blending_f(pinfo.in_ptr, pinfo.out_ptr, blend_target, blend_target + ref_offset as i64, out_target, r as f32);

        pre_ratio = ratio;

        blend_target -= next_pixel_step_in  as i64;
        out_target   -= next_pixel_step_out as i64;
    }
}
