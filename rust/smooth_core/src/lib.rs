// smooth_core Rust crate (Phase 2-C).
//
// Step 1: linkage probe (smooth_core_version).
// Step 2: preProcess ported.

mod preprocess;

use preprocess::{Pixel8, Pixel16, SmoothBbox, pre_process, SmoothPixel};

#[no_mangle]
pub extern "C" fn smooth_core_version() -> u32 {
    0x0002_0001
}

// SAFETY: Caller must ensure in_ptr is valid for rowbytes * height bytes and
// rowbytes is a multiple of sizeof(Pixel8) = 4. bbox_out must be non-null.
#[no_mangle]
pub unsafe extern "C" fn smooth_core_preprocess_u8(
    in_ptr: *mut Pixel8,
    rowbytes: i32,
    height: i32,
    is_white_trans: i32,
    bbox_out: *mut SmoothBbox,
) {
    preprocess_impl(in_ptr, rowbytes, height, is_white_trans, bbox_out);
}

// SAFETY: Caller must ensure in_ptr is valid for rowbytes * height bytes and
// rowbytes is a multiple of sizeof(Pixel16) = 8. bbox_out must be non-null.
#[no_mangle]
pub unsafe extern "C" fn smooth_core_preprocess_u16(
    in_ptr: *mut Pixel16,
    rowbytes: i32,
    height: i32,
    is_white_trans: i32,
    bbox_out: *mut SmoothBbox,
) {
    preprocess_impl(in_ptr, rowbytes, height, is_white_trans, bbox_out);
}

#[inline]
unsafe fn preprocess_impl<P: SmoothPixel>(
    in_ptr: *mut P,
    rowbytes: i32,
    height: i32,
    is_white_trans: i32,
    bbox_out: *mut SmoothBbox,
) {
    let width  = (rowbytes as usize) / core::mem::size_of::<P>();
    let height = height as usize;
    let slice  = core::slice::from_raw_parts_mut(in_ptr, width * height);
    let bb = pre_process(slice, width, height, is_white_trans != 0);
    *bbox_out = bb;
}
