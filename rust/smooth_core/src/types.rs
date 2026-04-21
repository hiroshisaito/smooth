// BlendingInfo / Cinfo / Params — Rust mirror of C++ define.h.
// Pixel types extend from preprocess::{Pixel8, Pixel16}.

use crate::preprocess::{Pixel8, Pixel16};

pub const CR_FLG_FILL:  u32 = 1 << 0;
pub const SECOND_COUNT: u32 = 1 << 0;

pub const BLEND_MODE_UP_H:   i32 = 0;
pub const BLEND_MODE_UP_V:   i32 = 1;
#[allow(dead_code)] pub const BLEND_MODE_DOWN_H: i32 = 2;
#[allow(dead_code)] pub const BLEND_MODE_DOWN_V: i32 = 3;

#[derive(Copy, Clone, Default)]
pub struct Cinfo {
    pub length: i64,   // long
    pub start:  f32,
    pub end:    f32,
    pub flg:    u32,
}

/// BlendingInfo<PixelType>. Holds raw pointers to in/out pixel buffers and scan state.
/// Caller must ensure the pointers outlive the BlendingInfo and that accesses are valid.
pub struct BlendingInfo<P: SmoothPixel> {
    pub in_ptr:  *mut P,   // in-place preProcess edits, reads by helpers
    pub out_ptr: *mut P,
    pub width:         i32,
    pub logical_width: i32,
    pub height:        i32,
    pub rowbytes:      i32,

    pub i: i32,
    pub j: i32,
    pub in_target:  i64,   // long
    pub out_target: i64,
    pub core: [Cinfo; 4],
    pub flag: i32,
    pub range: u32,
    pub mode: i32,
    pub line_weight: f32,
}

impl<P: SmoothPixel> Clone for BlendingInfo<P> {
    fn clone(&self) -> Self {
        Self {
            in_ptr: self.in_ptr,
            out_ptr: self.out_ptr,
            width: self.width,
            logical_width: self.logical_width,
            height: self.height,
            rowbytes: self.rowbytes,
            i: self.i,
            j: self.j,
            in_target: self.in_target,
            out_target: self.out_target,
            core: self.core,
            flag: self.flag,
            range: self.range,
            mode: self.mode,
            line_weight: self.line_weight,
        }
    }
}

/// SmoothPixel trait: extended from preprocess to include arithmetic accessors.
pub trait SmoothPixel: Copy + PartialEq + 'static {
    fn white_key() -> Self;
    fn null_pixel() -> Self;
    fn rgb_eq(&self, other: &Self) -> bool;
    fn alpha_is_zero(&self) -> bool;

    /// `ABS(R0-R1) + ABS(G0-G1) + ABS(B0-B1) + ABS(A0-A1)` as u32 — absolute-diff sum.
    fn delta_sum(&self, other: &Self) -> u32;

    /// max_value (0xFF for u8, 0x8000 for u16) as u32.
    fn max_value() -> u32;

    fn red(&self)   -> u32;
    fn green(&self) -> u32;
    fn blue(&self)  -> u32;
    fn alpha(&self) -> u32;

    fn set_red(&mut self,   v: u32);
    fn set_green(&mut self, v: u32);
    fn set_blue(&mut self,  v: u32);
    fn set_alpha(&mut self, v: u32);

    /// For FAST_COMPARE packed comparison — packed pixel as u64.
    fn as_packed(&self) -> u64;
}

impl SmoothPixel for Pixel8 {
    #[inline(always)] fn white_key()  -> Self { Pixel8 { alpha: 0xFF, red: 0xFF, green: 0xFF, blue: 0xFF } }
    #[inline(always)] fn null_pixel() -> Self { Pixel8 { alpha: 0,    red: 0,    green: 0,    blue: 0    } }
    #[inline(always)] fn rgb_eq(&self, o: &Self) -> bool { self.red == o.red && self.green == o.green && self.blue == o.blue }
    #[inline(always)] fn alpha_is_zero(&self) -> bool { self.alpha == 0 }

    #[inline(always)]
    fn delta_sum(&self, o: &Self) -> u32 {
        self.red.abs_diff(o.red)     as u32
            + self.green.abs_diff(o.green) as u32
            + self.blue.abs_diff(o.blue)   as u32
            + self.alpha.abs_diff(o.alpha) as u32
    }
    #[inline(always)] fn max_value() -> u32 { 0xFF }

    #[inline(always)] fn red(&self)   -> u32 { self.red   as u32 }
    #[inline(always)] fn green(&self) -> u32 { self.green as u32 }
    #[inline(always)] fn blue(&self)  -> u32 { self.blue  as u32 }
    #[inline(always)] fn alpha(&self) -> u32 { self.alpha as u32 }

    #[inline(always)] fn set_red(&mut self,   v: u32) { self.red   = v as u8; }
    #[inline(always)] fn set_green(&mut self, v: u32) { self.green = v as u8; }
    #[inline(always)] fn set_blue(&mut self,  v: u32) { self.blue  = v as u8; }
    #[inline(always)] fn set_alpha(&mut self, v: u32) { self.alpha = v as u8; }

    #[inline(always)]
    fn as_packed(&self) -> u64 {
        ((self.alpha as u64)) | ((self.red as u64) << 8) | ((self.green as u64) << 16) | ((self.blue as u64) << 24)
    }
}

impl SmoothPixel for Pixel16 {
    #[inline(always)] fn white_key()  -> Self { Pixel16 { alpha: 0x8000, red: 0x8000, green: 0x8000, blue: 0x8000 } }
    #[inline(always)] fn null_pixel() -> Self { Pixel16 { alpha: 0,      red: 0,      green: 0,      blue: 0      } }
    #[inline(always)] fn rgb_eq(&self, o: &Self) -> bool { self.red == o.red && self.green == o.green && self.blue == o.blue }
    #[inline(always)] fn alpha_is_zero(&self) -> bool { self.alpha == 0 }

    #[inline(always)]
    fn delta_sum(&self, o: &Self) -> u32 {
        self.red.abs_diff(o.red)     as u32
            + self.green.abs_diff(o.green) as u32
            + self.blue.abs_diff(o.blue)   as u32
            + self.alpha.abs_diff(o.alpha) as u32
    }
    #[inline(always)] fn max_value() -> u32 { 0x8000 }

    #[inline(always)] fn red(&self)   -> u32 { self.red   as u32 }
    #[inline(always)] fn green(&self) -> u32 { self.green as u32 }
    #[inline(always)] fn blue(&self)  -> u32 { self.blue  as u32 }
    #[inline(always)] fn alpha(&self) -> u32 { self.alpha as u32 }

    #[inline(always)] fn set_red(&mut self,   v: u32) { self.red   = v as u16; }
    #[inline(always)] fn set_green(&mut self, v: u32) { self.green = v as u16; }
    #[inline(always)] fn set_blue(&mut self,  v: u32) { self.blue  = v as u16; }
    #[inline(always)] fn set_alpha(&mut self, v: u32) { self.alpha = v as u16; }

    #[inline(always)]
    fn as_packed(&self) -> u64 {
        (self.alpha as u64) | ((self.red as u64) << 16) | ((self.green as u64) << 32) | ((self.blue as u64) << 48)
    }
}


// Safe accessors on a raw-pointer buffer. Callers ensure `offset` is in-bounds.
/// # Safety
/// `ptr.add(offset)` must be within the allocated buffer and dereferenceable.
#[inline(always)]
pub unsafe fn px_read<P: Copy>(ptr: *const P, offset: i64) -> P {
    *ptr.offset(offset as isize)
}

/// # Safety
/// `ptr.add(offset)` must be within the allocated buffer and mutably dereferenceable;
/// caller must ensure no other reference aliases the same slot during the write.
#[inline(always)]
pub unsafe fn px_write<P: Copy>(ptr: *mut P, offset: i64, value: P) {
    *ptr.offset(offset as isize) = value;
}
