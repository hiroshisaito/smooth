// ComparePixel / ComparePixelEqual — ported from util.h macros.

use crate::types::{BlendingInfo, SmoothPixel, px_read};

/// `(ABS(R0-R1)+ABS(G0-G1)+ABS(B0-B1)+ABS(A0-A1)) > range` — returns true when
/// the two pixels are "different" (beyond the tolerance `info.range`).
#[inline(always)]
pub unsafe fn compare_pixel<P: SmoothPixel>(info: &BlendingInfo<P>, p0: i64, p1: i64) -> bool {
    let a = px_read(info.in_ptr, p0);
    let b = px_read(info.in_ptr, p1);
    a.delta_sum(&b) > info.range
}

/// `(ABS(...)+...) <= range` — complement of compare_pixel.
#[inline(always)]
pub unsafe fn compare_pixel_equal<P: SmoothPixel>(info: &BlendingInfo<P>, p0: i64, p1: i64) -> bool {
    let a = px_read(info.in_ptr, p0);
    let b = px_read(info.in_ptr, p1);
    a.delta_sum(&b) <= info.range
}

/// FAST_COMPARE_PIXEL: packed compare, returns true if bytes differ.
///
/// Mirrors the C++ `FAST_COMPARE_PIXEL` macro: reinterpret the pixel bytes as a
/// u32 (8bpc) or u64 (16bpc) and compare directly. The earlier Rust version
/// materialised the packed value via four shift+OR operations on struct fields;
/// the compiler did not reliably fold that back to a single load, leaving the
/// scan loop ~1.7x slower than the C++ baseline. The size branch below
/// monomorphises per Pixel type so the non-matching arm is compiled out.
///
/// # Safety
/// `info.in_ptr` must point to a buffer of `P` values with at least
/// `max(p0, p1) + 1` elements, and the buffer must be properly aligned for `P`.
#[inline(always)]
pub unsafe fn fast_compare_pixel<P: SmoothPixel>(info: &BlendingInfo<P>, p0: i64, p1: i64) -> bool {
    let base = info.in_ptr;
    match core::mem::size_of::<P>() {
        4 => {
            let a = *(base.offset(p0 as isize) as *const u32);
            let b = *(base.offset(p1 as isize) as *const u32);
            a != b
        }
        8 => {
            let a = *(base.offset(p0 as isize) as *const u64);
            let b = *(base.offset(p1 as isize) as *const u64);
            a != b
        }
        _ => {
            // Fallback for hypothetical other pixel sizes; not exercised by
            // Pixel8 / Pixel16. Keeps the function total without panic.
            let a = px_read(info.in_ptr, p0);
            let b = px_read(info.in_ptr, p1);
            a.as_packed() != b.as_packed()
        }
    }
}
