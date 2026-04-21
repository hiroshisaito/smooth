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
    pub parallel:      i32,      // 0 = serial, 1 = rayon strip-parallel
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
    // Snapshot all plain-value fields so the parallel closure does not capture
    // the !Sync raw pointers in RowRangeArgs directly.
    let j_start       = a.j_start;
    let j_end         = a.j_end;
    let i_start       = a.i_start;
    let i_end         = a.i_end;
    let width         = a.width;
    let logical_width = a.logical_width;
    let height        = a.height;
    let rowbytes      = a.rowbytes;
    let range         = a.range;
    let line_weight   = a.line_weight;
    let in_addr       = a.in_ptr  as usize;
    let out_addr      = a.out_ptr as usize;

    let build_info = |in_addr: usize, out_addr: usize| BlendingInfo::<P> {
        in_ptr:        in_addr  as *mut P,
        out_ptr:       out_addr as *mut P,
        width, logical_width, height, rowbytes,
        i: 0, j: 0,
        in_target: 0, out_target: 0,
        core: [Cinfo::default(); 4],
        flag: 0,
        range,
        mode: 0,
        line_weight,
    };

    let rows = j_end - j_start;
    let nthreads = if a.parallel == 0 { 1 } else { rayon::current_num_threads() as i32 };

    // Phase 1 compatible thresholds: small images / single-core → serial.
    if nthreads <= 1 || rows < 32 {
        let tmpl = build_info(in_addr, out_addr);
        process_row_range(&tmpl, j_start, j_end, i_start, i_end);
        return;
    }

    let rows_per_thread = (rows + nthreads - 1) / nthreads;

    use rayon::iter::{IntoParallelIterator, ParallelIterator};
    (0..nthreads).into_par_iter().for_each(|t| {
        let start = j_start + t * rows_per_thread;
        let end   = (start + rows_per_thread).min(j_end);
        if start >= end { return; }
        let tmpl = BlendingInfo::<P> {
            in_ptr:        in_addr  as *mut P,
            out_ptr:       out_addr as *mut P,
            width, logical_width, height, rowbytes,
            i: 0, j: 0,
            in_target: 0, out_target: 0,
            core: [Cinfo::default(); 4],
            flag: 0,
            range,
            mode: 0,
            line_weight,
        };
        unsafe { process_row_range(&tmpl, start, end, i_start, i_end); }
    });
}
