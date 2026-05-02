// Phase 2-C Step 2: preProcess() ported from C++ smooth_core.h.
//
// Layout assumption: PixelN is `{ alpha, red, green, blue }` in memory order,
// matching AE SDK's PF_Pixel / PF_Pixel16 definitions in AE_Effect.h.

#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Pixel8 {
    pub alpha: u8,
    pub red:   u8,
    pub green: u8,
    pub blue:  u8,
}

#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Pixel16 {
    pub alpha: u16,
    pub red:   u16,
    pub green: u16,
    pub blue:  u16,
}

/// PF_PixelFloat layout: `{ alpha, red, green, blue }` of f32 in AE's 0.0–1.0
/// domain. Phase 2-A.2 Step 1 added f32 path for 32bpc support.
/// `Eq` is intentionally NOT derived — f32 has NaN.
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Pixel32 {
    pub alpha: f32,
    pub red:   f32,
    pub green: f32,
    pub blue:  f32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SmoothBbox {
    pub top:    i32,
    pub left:   i32,
    pub right:  i32,
    pub bottom: i32,
}

use crate::types::SmoothPixel;

pub fn pre_process<P: SmoothPixel>(
    pixels: &mut [P],
    width: usize,
    height: usize,
    is_white_trans: bool,
) -> SmoothBbox {
    let key  = P::white_key();
    let null = P::null_pixel();

    let mut top: i32    = 0;
    let mut left: i32   = width as i32;
    let mut right: i32  = 0;
    let mut bottom: i32 = 0;
    let mut top_found  = false;
    let mut left_found = false;

    let mut t: usize = 0;

    if is_white_trans {
        for j in 0..height {
            if !top_found { top = j as i32; }
            for i in 0..width {
                let p = pixels[t];
                if p.rgb_eq(&key) {
                    pixels[t] = null;
                } else if p.alpha_is_zero() {
                    // already transparent, skip bbox update
                } else {
                    top_found  = true;
                    left_found = true;
                    let ii = i as i32;
                    let jj = j as i32;
                    if left > ii { left = ii; }
                    if right < ii { right = ii; }
                    if bottom < jj { bottom = jj; }
                }
                t += 1;
            }
        }
    } else {
        for j in 0..height {
            if !top_found { top = j as i32; }
            for i in 0..width {
                let p = pixels[t];
                if !p.rgb_eq(&key) && !p.alpha_is_zero() {
                    top_found  = true;
                    left_found = true;
                    let ii = i as i32;
                    let jj = j as i32;
                    if left > ii { left = ii; }
                    if right < ii { right = ii; }
                    if bottom < jj { bottom = jj; }
                }
                t += 1;
            }
        }
    }

    SmoothBbox {
        top:    if top_found  { top  } else { 0 },
        left:   if left_found { left } else { 0 },
        right:  right + 1,
        bottom: bottom + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pixel8(a: u8, r: u8, g: u8, b: u8) -> Pixel8 { Pixel8 { alpha: a, red: r, green: g, blue: b } }

    #[test]
    fn all_transparent_returns_origin_bbox() {
        let mut img = vec![make_pixel8(0, 0, 0, 0); 4 * 3];
        let bb = pre_process(&mut img, 4, 3, false);
        assert_eq!(bb.top, 0);
        assert_eq!(bb.left, 0);
        assert_eq!(bb.right, 1);
        assert_eq!(bb.bottom, 1);
    }

    #[test]
    fn white_gets_replaced_when_enabled() {
        let w  = make_pixel8(0xFF, 0xFF, 0xFF, 0xFF);
        let r  = make_pixel8(0xFF, 0xFF, 0x00, 0x00);
        let mut img = vec![w, w, w, w, r, w];
        let bb = pre_process(&mut img, 3, 2, true);
        for idx in [0, 1, 2, 3, 5] {
            assert_eq!(img[idx], make_pixel8(0, 0, 0, 0), "idx {idx}");
        }
        assert_eq!(img[4], r);
        assert_eq!(bb.top,    1);
        assert_eq!(bb.left,   1);
        assert_eq!(bb.right,  2);
        assert_eq!(bb.bottom, 2);
    }

    #[test]
    fn white_kept_when_disabled_bbox_spans_non_white() {
        let w = make_pixel8(0xFF, 0xFF, 0xFF, 0xFF);
        let r = make_pixel8(0xFF, 0xFF, 0x00, 0x00);
        let mut img = vec![w, w, w, w, r, w];
        let bb = pre_process(&mut img, 3, 2, false);
        assert_eq!(img[0], w);
        assert_eq!(img[4], r);
        assert_eq!(bb.top,    1);
        assert_eq!(bb.left,   1);
        assert_eq!(bb.right,  2);
        assert_eq!(bb.bottom, 2);
    }

    // -- Phase 2-A.2 Step 1: 32bpc (Pixel32, f32) preprocess tests --

    fn px32(a: f32, r: f32, g: f32, b: f32) -> Pixel32 {
        Pixel32 { alpha: a, red: r, green: g, blue: b }
    }

    #[test]
    fn pixel32_all_transparent_returns_origin_bbox() {
        let mut img = vec![px32(0.0, 0.0, 0.0, 0.0); 4 * 3];
        let bb = pre_process(&mut img, 4, 3, false);
        assert_eq!(bb.top, 0);
        assert_eq!(bb.left, 0);
        assert_eq!(bb.right, 1);
        assert_eq!(bb.bottom, 1);
    }

    #[test]
    fn pixel32_white_gets_replaced_when_enabled() {
        let w = px32(1.0, 1.0, 1.0, 1.0);
        let r = px32(1.0, 1.0, 0.0, 0.0);
        let mut img = vec![w, w, w, w, r, w];
        let bb = pre_process(&mut img, 3, 2, true);
        for idx in [0, 1, 2, 3, 5] {
            assert_eq!(img[idx], px32(0.0, 0.0, 0.0, 0.0), "idx {idx}");
        }
        assert_eq!(img[4], r);
        assert_eq!(bb.top,    1);
        assert_eq!(bb.left,   1);
        assert_eq!(bb.right,  2);
        assert_eq!(bb.bottom, 2);
    }

    #[test]
    fn pixel32_overbright_does_not_crash_or_produce_nan() {
        // AE 32bpc allows overbright (>1.0). preprocess must not equate
        // overbright to white_key (1.0,1.0,1.0,1.0) and must not produce
        // NaN or Inf.
        let overbright = px32(1.0, 2.5, 3.0, 1.5);  // alpha=1.0 still
        let r          = px32(1.0, 1.0, 0.0, 0.0);
        let mut img = vec![overbright, overbright, overbright, overbright, r, overbright];
        let bb = pre_process(&mut img, 3, 2, true);

        // Overbright is not white_key, so even with white_option enabled
        // those pixels stay (they don't match the exact (1,1,1,1) key).
        for idx in [0, 1, 2, 3, 5] {
            assert_eq!(img[idx], overbright, "idx {idx} should remain overbright");
            assert!(img[idx].red.is_finite() && img[idx].red.is_finite());
            assert!(!img[idx].red.is_nan() && !img[idx].green.is_nan()
                 && !img[idx].blue.is_nan() && !img[idx].alpha.is_nan());
        }
        assert_eq!(img[4], r);
        // bbox spans the whole image since overbright != white_key, so it's
        // treated as non-white content.
        assert!(bb.right  > bb.left);
        assert!(bb.bottom > bb.top);
    }

    #[test]
    fn pixel32_nan_inputs_do_not_propagate_to_alpha_zero_logic() {
        // NaN poisoning defense: even if the upstream feeds NaN, we should
        // not crash. PartialEq with NaN returns false, so NaN pixels are
        // treated as "not white_key, not null", so preprocess leaves them
        // untouched.
        let nan = px32(f32::NAN, f32::NAN, f32::NAN, f32::NAN);
        let r   = px32(1.0, 1.0, 0.0, 0.0);
        let mut img = vec![nan, nan, nan, nan, r, nan];
        let _bb = pre_process(&mut img, 3, 2, true);
        // We mostly care that it doesn't panic. Spot-check that NaN slots
        // remain NaN (PartialEq miss => no replacement).
        assert!(img[0].red.is_nan());
        assert_eq!(img[4], r);
    }

    #[test]
    fn pixel32_subnormal_inputs_handled() {
        // Subnormal f32 (very small) values must not cause issues.
        let sub = px32(f32::MIN_POSITIVE / 2.0, f32::MIN_POSITIVE / 2.0,
                       f32::MIN_POSITIVE / 2.0, f32::MIN_POSITIVE / 2.0);
        let mut img = vec![sub; 4 * 3];
        let bb = pre_process(&mut img, 4, 3, false);
        // Subnormals are non-zero, non-white. alpha is non-zero so they're
        // not stripped. bbox should span (since alpha != 0 → "found" pixels).
        assert!(bb.right >= bb.left);
    }
}
