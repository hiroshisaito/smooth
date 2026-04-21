// ComparePixel / ComparePixelEqual — ported from util.h macros.

use crate::types::{BlendingInfo, SmoothPixel, px_read};

/// `(ABS(R0-R1)+ABS(G0-G1)+ABS(B0-B1)+ABS(A0-A1)) > range` — returns true when
/// the two pixels are "different" (beyond the tolerance `info.range`).
#[inline]
pub unsafe fn compare_pixel<P: SmoothPixel>(info: &BlendingInfo<P>, p0: i64, p1: i64) -> bool {
    let a = px_read(info.in_ptr, p0);
    let b = px_read(info.in_ptr, p1);
    a.delta_sum(&b) > info.range
}

/// `(ABS(...)+...) <= range` — complement of compare_pixel.
#[inline]
pub unsafe fn compare_pixel_equal<P: SmoothPixel>(info: &BlendingInfo<P>, p0: i64, p1: i64) -> bool {
    let a = px_read(info.in_ptr, p0);
    let b = px_read(info.in_ptr, p1);
    a.delta_sum(&b) <= info.range
}

/// FAST_COMPARE_PIXEL: packed compare, returns true if bytes differ.
#[inline]
pub unsafe fn fast_compare_pixel<P: SmoothPixel>(info: &BlendingInfo<P>, p0: i64, p1: i64) -> bool {
    let a = px_read(info.in_ptr, p0);
    let b = px_read(info.in_ptr, p1);
    a.as_packed() != b.as_packed()
}
