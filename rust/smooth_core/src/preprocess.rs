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
}
