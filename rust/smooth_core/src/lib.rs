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
// Phase 2-A.3 Sub-stage B scaffold (RFC §6.1).
// Trait + module tree only; real Metal/CUDA dispatch arrives in Sub-stage C/E.
mod gpu;

use preprocess::{Pixel8, Pixel16, SmoothBbox, pre_process};
use types::{BlendingInfo, Cinfo, SmoothPixel};
use process::process_row_range;

#[no_mangle]
pub extern "C" fn smooth_core_version() -> u32 {
    // 0x0002_0003: added smooth_core_build_id() (backwards compatible — old callers keep working).
    0x0002_0003
}

/// Human-readable build identity, captured at Rust crate build time by
/// `build.rs`. Format: `<CARGO_PKG_VERSION>+<git-short-sha>[+dirty]`.
/// The trailing \0 makes it a valid null-terminated C string.
static BUILD_ID: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "+",
    env!("SMOOTH_CORE_GIT_SHA"),
    "\0",
);

/// Returns a pointer to a static null-terminated ASCII string describing the
/// Rust crate build (crate semver + git short SHA + optional `+dirty`).
/// The pointer is valid for the lifetime of the process; callers must NOT
/// free it.
#[no_mangle]
pub extern "C" fn smooth_core_build_id() -> *const core::ffi::c_char {
    BUILD_ID.as_ptr() as *const core::ffi::c_char
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

/// Wrapper that lets the in/out pixel buffer pointers cross thread boundaries
/// under `rayon::into_par_iter`.
///
/// # Safety invariant carried by the type (not by the compiler)
///
/// Multiple rayon workers dereference the same `*mut P` addresses stored here.
/// That is sound only because Phase 1 established the following write-pattern
/// invariant and Phase 2-C preserves it byte-for-byte:
///
/// * `in_ptr` is **read-only** after `smooth_core::process` finishes its
///   `preProcess` + `memcpy(out, in)` phase. Workers never write to `in_ptr`.
/// * `out_ptr` is written concurrently by workers, each handling a disjoint
///   row strip `[j_start, j_end)`. Writes from up/downMode blending at strip
///   boundaries can overlap by up to a few rows (Phase 1 `SEAM_HALO=0`); the
///   resulting boundary residual is bounded, deterministic only up to thread
///   scheduling, and intentionally accepted as NEAR-IDENTICAL in regression
///   (~30 bytes per HD 16bpc frame).
///
/// This means the parallel path is **technically a data race under Rust's
/// aliasing rules**, just as the C++ `std::thread` version was. The race is
/// benign in the sense that: no pointer dereference is ever out of bounds; no
/// object is partially written (writes are whole-pixel `*mut P = value`); and
/// both strands converge on the same final pixel contents for non-boundary
/// rows. A future revision that adds a proper halo pass (SEAM_HALO > 0) or
/// moves to a tile-based model will replace this `unsafe impl Sync`.
///
/// Using this wrapper instead of raw `usize`-cast pointers makes the contract
/// explicit; if someone later changes a worker to write into `in_ptr` or lets
/// strips grow unbounded, they have to touch this type and see the comment.
#[derive(Copy, Clone)]
struct SharedBuf<P> {
    in_ptr:  *mut P,
    out_ptr: *mut P,
}
// SAFETY: see the doc comment above. The contract is enforced by design, not
// by the compiler.
unsafe impl<P> Send for SharedBuf<P> {}
unsafe impl<P> Sync for SharedBuf<P> {}

#[inline]
unsafe fn run_row_range<P: SmoothPixel>(a: &RowRangeArgs) {
    // Snapshot plain-value fields and wrap the raw pointers in SharedBuf so the
    // parallel closure stays Send + Sync with the contract made explicit.
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
    let buf: SharedBuf<P> = SharedBuf {
        in_ptr:  a.in_ptr  as *mut P,
        out_ptr: a.out_ptr as *mut P,
    };

    let build_info = |buf: SharedBuf<P>| BlendingInfo::<P> {
        in_ptr:  buf.in_ptr,
        out_ptr: buf.out_ptr,
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
        let tmpl = build_info(buf);
        process_row_range(&tmpl, j_start, j_end, i_start, i_end);
        return;
    }

    let rows_per_thread = (rows + nthreads - 1) / nthreads;

    use rayon::iter::{IntoParallelIterator, ParallelIterator};
    (0..nthreads).into_par_iter().for_each(|t| {
        let start = j_start + t * rows_per_thread;
        let end   = (start + rows_per_thread).min(j_end);
        if start >= end { return; }
        let tmpl = build_info(buf);
        // SAFETY: see SharedBuf doc comment for the concurrent-write contract.
        unsafe { process_row_range(&tmpl, start, end, i_start, i_end); }
    });
}
