// BlendingInfo / Cinfo / Params / SmoothScalar / SmoothPixel.
// Pixel types extend from preprocess::{Pixel8, Pixel16, Pixel32}.

use std::ops::{Add, AddAssign, Div, Mul, Sub};

use crate::preprocess::{Pixel8, Pixel16, Pixel32};

// ---------------------------------------------------------------------------
// SmoothScalar (Phase 2-A.2 Step 1)
// ---------------------------------------------------------------------------
// Numeric domain trait. u8/u16 pixels use `u32` (fixed-point integer math
// scaled by max_value()); 32bpc PF_PixelFloat uses `f32` directly with
// max_value() = 1.0, so the same `output = (target*alpha + ref*(max-alpha)) / max`
// formula reduces to clean float blending without integer rounding.
pub trait SmoothScalar:
    Copy
    + PartialOrd
    + PartialEq
    + Default
    + Add<Output = Self>
    + AddAssign
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Send
    + Sync
    + 'static
{
    /// Numeric zero in this domain.
    fn zero() -> Self;
    /// Map a `[0, 1]` ratio into the pixel-type's scalar domain, given the
    /// type's `max_value`. For u32: `(max * ratio) as u32` (integer fixed
    /// point). For f32: just `ratio` (max is always 1.0).
    fn from_ratio_with_max(ratio: f32, max: Self) -> Self;
    /// Promote a small integer count (e.g. divisor for averaging) to this
    /// scalar domain. For u32 this is identity; for f32 it's a cast.
    fn from_u32(n: u32) -> Self;
    /// `self / n` honouring the scalar's domain (integer division for u32,
    /// float division for f32). Used for averaging like `(a+b+c) / 3` in
    /// lack/link8.
    fn div_by_int(self, n: u32) -> Self;
}

impl SmoothScalar for u32 {
    #[inline(always)] fn zero() -> Self { 0 }
    #[inline(always)]
    fn from_ratio_with_max(ratio: f32, max: u32) -> Self { (max as f32 * ratio) as u32 }
    #[inline(always)] fn from_u32(n: u32) -> Self { n }
    #[inline(always)] fn div_by_int(self, n: u32) -> Self { self / n }
}

impl SmoothScalar for f32 {
    #[inline(always)] fn zero() -> Self { 0.0 }
    #[inline(always)]
    fn from_ratio_with_max(ratio: f32, _max: f32) -> Self { ratio }
    #[inline(always)] fn from_u32(n: u32) -> Self { n as f32 }
    #[inline(always)] fn div_by_int(self, n: u32) -> Self { self / (n as f32) }
}

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
    pub range: P::Scalar,
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
/// Phase 2-A.2 Step 1 introduced `type Scalar` so 32bpc (PF_PixelFloat, f32)
/// can share the same algorithm code as 8/16bpc (u32 fixed-point).
pub trait SmoothPixel: Copy + PartialEq + 'static {
    /// Numeric domain for delta_sum, max_value, range, channel accessors.
    /// `u32` for u8/u16 (integer fixed-point), `f32` for 32bpc.
    type Scalar: SmoothScalar;

    fn white_key() -> Self;
    fn null_pixel() -> Self;
    fn rgb_eq(&self, other: &Self) -> bool;
    fn alpha_is_zero(&self) -> bool;

    /// `ABS(R0-R1) + ABS(G0-G1) + ABS(B0-B1) + ABS(A0-A1)` in this pixel's
    /// scalar domain. For 32bpc with overbright inputs this can exceed 4.0.
    fn delta_sum(&self, other: &Self) -> Self::Scalar;

    /// `0xFF` (u8) / `0x8000` (u16) / `1.0` (f32). Used as denominator in
    /// blend math (fixed-point unit for u32 path; identity for f32 path).
    fn max_value() -> Self::Scalar;

    fn red(&self)   -> Self::Scalar;
    fn green(&self) -> Self::Scalar;
    fn blue(&self)  -> Self::Scalar;
    fn alpha(&self) -> Self::Scalar;

    fn set_red(&mut self,   v: Self::Scalar);
    fn set_green(&mut self, v: Self::Scalar);
    fn set_blue(&mut self,  v: Self::Scalar);
    fn set_alpha(&mut self, v: Self::Scalar);

    /// For FAST_COMPARE packed comparison — packed pixel as u64.
    /// f32 pixels use bit-pattern packing (4×f32 → u128 won't fit, so f32
    /// path falls back to per-channel compare; see compare::fast_compare_*).
    fn as_packed(&self) -> u64;
}

impl SmoothPixel for Pixel8 {
    type Scalar = u32;

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
    type Scalar = u32;

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

impl SmoothPixel for Pixel32 {
    type Scalar = f32;

    #[inline(always)] fn white_key()  -> Self { Pixel32 { alpha: 1.0, red: 1.0, green: 1.0, blue: 1.0 } }
    #[inline(always)] fn null_pixel() -> Self { Pixel32 { alpha: 0.0, red: 0.0, green: 0.0, blue: 0.0 } }
    #[inline(always)] fn rgb_eq(&self, o: &Self) -> bool {
        self.red == o.red && self.green == o.green && self.blue == o.blue
    }
    #[inline(always)] fn alpha_is_zero(&self) -> bool { self.alpha == 0.0 }

    #[inline(always)]
    fn delta_sum(&self, o: &Self) -> f32 {
        (self.red   - o.red).abs()
            + (self.green - o.green).abs()
            + (self.blue  - o.blue).abs()
            + (self.alpha - o.alpha).abs()
    }
    #[inline(always)] fn max_value() -> f32 { 1.0 }

    #[inline(always)] fn red(&self)   -> f32 { self.red }
    #[inline(always)] fn green(&self) -> f32 { self.green }
    #[inline(always)] fn blue(&self)  -> f32 { self.blue }
    #[inline(always)] fn alpha(&self) -> f32 { self.alpha }

    #[inline(always)] fn set_red(&mut self,   v: f32) { self.red   = v; }
    #[inline(always)] fn set_green(&mut self, v: f32) { self.green = v; }
    #[inline(always)] fn set_blue(&mut self,  v: f32) { self.blue  = v; }
    #[inline(always)] fn set_alpha(&mut self, v: f32) { self.alpha = v; }

    /// Packed u64: bit-cast of (alpha, red) as two u32 bit patterns. Not
    /// usable for fast_compare across all four channels (would need u128);
    /// the f32 compare path falls back to per-channel comparisons.
    #[inline(always)]
    fn as_packed(&self) -> u64 {
        (self.alpha.to_bits() as u64) | ((self.red.to_bits() as u64) << 32)
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
