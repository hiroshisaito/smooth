// smooth_core Rust crate (Phase 2-C).
// Step 1: linkage probe.
// Step 2: preProcess ported.
// Step 3: helpers + process_row_range ported (serial).

mod preprocess;
mod types;
mod compare;
mod blend;
mod lack;
mod up_mode;
mod down_mode;
mod link8;
mod process;

use preprocess::{Pixel8, Pixel16, SmoothBbox, pre_process};
use types::{BlendingInfo, Cinfo, SmoothPixel};
use process::process_row_range;

#[no_mangle]
pub extern "C" fn smooth_core_version() -> u32 {
    0x0002_0002
}

// --- preProcess FFI (Step 2) ---

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

// --- process_row_range FFI (Step 3) ---

#[repr(C)]
pub struct RowRangeArgs {
    pub in_ptr:        *mut u8,  // pointer to pixel 0 (interpreted per bpc)
    pub out_ptr:       *mut u8,
    pub width:         i32,      // rowbytes / sizeof(Pixel)
    pub logical_width: i32,
    pub height:        i32,
    pub rowbytes:      i32,
    pub range:         u32,
    pub line_weight:   f32,
    pub j_start:       i32,
    pub j_end:         i32,
    pub i_start:       i32,
    pub i_end:         i32,
}

#[no_mangle]
pub unsafe extern "C" fn smooth_core_process_row_range_u8(args: *const RowRangeArgs) {
    run_row_range::<Pixel8>(&*args);
}

#[no_mangle]
pub unsafe extern "C" fn smooth_core_process_row_range_u16(args: *const RowRangeArgs) {
    run_row_range::<Pixel16>(&*args);
}

#[inline]
unsafe fn run_row_range<P: SmoothPixel>(a: &RowRangeArgs) {
    let template = BlendingInfo::<P> {
        in_ptr:        a.in_ptr  as *mut P,
        out_ptr:       a.out_ptr as *mut P,
        width:         a.width,
        logical_width: a.logical_width,
        height:        a.height,
        rowbytes:      a.rowbytes,
        i: 0, j: 0,
        in_target: 0, out_target: 0,
        core: [Cinfo::default(); 4],
        flag: 0,
        range: a.range,
        mode:  0,
        line_weight: a.line_weight,
    };
    process_row_range(&template, a.j_start, a.j_end, a.i_start, a.i_end);
}
