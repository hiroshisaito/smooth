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

use preprocess::{Pixel8, Pixel16, Pixel32, SmoothBbox, pre_process};
use types::{BlendingInfo, Cinfo, SmoothPixel};
use process::process_row_range;

#[no_mangle]
pub extern "C" fn smooth_core_version() -> u32 {
    // 0x0002_0003: added smooth_core_build_id() (backwards compatible — old callers keep working).
    // 0x0002_0004: added GPU plumbing FFI (uuid_new / fallen state / backend_usable
    //              / force-error injection). Still backward compatible — old plugin
    //              binaries that never call these symbols continue to load.
    // 0x0002_0005: added Mac Metal dispatch FFI (smooth_core_metal_{create,destroy,
    //              dispatch_passthrough}) for Sub-stage C-2.5a. Mac-only symbols; the
    //              Windows staticlib does not export them. Still backward compatible.
    // 0x0002_0006: added smooth_core_metal_dispatch_preprocess (white-key strip kernel)
    //              for Sub-stage C-2.5b.1. Production GPU path now uses preprocess
    //              instead of passthrough.
    // 0x0002_0007: added smooth_core_metal_dispatch_smooth_chain (preprocess + detect
    //              + blend in a single command buffer) for Sub-stage C-2.5b.2-prep2a.
    //              The blend kernel currently handles only mode_flg=15 (link8_square
    //              centre); other modes pass through unchanged.
    // 0x0002_0008: dispatch_smooth_chain accepts two uint32-per-pixel priority buffer
    //              pointers (priority_v, priority_h) for Sub-stage C-2.5b.2-prep2b.2a.
    //              The priority init kernel zeros them at the start of every dispatch;
    //              follow-up commits wire claim+apply kernels for line-level blends.
    // 0x0002_000a: dispatch_smooth_chain takes line_weight (f32) and runs the
    //              mode_flg=15 outside line-blend claim+apply kernels for prep2b.2b
    //              (tile-dispatch retry; commit ac408f7's monolithic version was
    //              reverted as 3cea31b due to GPU watchdog timeout under MFR+4K).
    //              CPU semantics ported from link8_square_blend_outside; atomic_min
    //              priority resolution per design memo §6.
    0x0002_000a
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

/// Phase 2-A.2 Step 1: 32bpc (PF_PixelFloat) preprocess entry point.
#[no_mangle]
pub unsafe extern "C" fn smooth_core_preprocess_f32(
    in_ptr: *mut Pixel32,
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
    let a = &*args;
    run_row_range::<Pixel8>(a, a.range);
}

#[no_mangle]
pub unsafe extern "C" fn smooth_core_process_row_range_u16(args: *const RowRangeArgs) {
    let a = &*args;
    run_row_range::<Pixel16>(a, a.range);
}

/// Phase 2-A.2 Step 1: 32bpc (PF_PixelFloat) entry point. Mirrors u8/u16
/// FFI but passes `range` as f32 (raw slider value × max_value=1.0 already
/// applied on the C++ side).
#[repr(C)]
pub struct RowRangeArgsF32 {
    pub in_ptr:        *mut u8,
    pub out_ptr:       *mut u8,
    pub width:         i32,
    pub logical_width: i32,
    pub height:        i32,
    pub rowbytes:      i32,
    pub range:         f32,      // f32 instead of u32 for 32bpc
    pub line_weight:   f32,
    pub j_start:       i32,
    pub j_end:         i32,
    pub i_start:       i32,
    pub i_end:         i32,
    pub parallel:      i32,
}

#[no_mangle]
pub unsafe extern "C" fn smooth_core_process_row_range_f32(args: *const RowRangeArgsF32) {
    let a = &*args;
    // Reuse the same shape; only `range` differs in scalar type. Build a
    // `RowRangeArgs` view for the shared fields and pass `range` separately.
    let shared = RowRangeArgs {
        in_ptr: a.in_ptr,
        out_ptr: a.out_ptr,
        width: a.width,
        logical_width: a.logical_width,
        height: a.height,
        rowbytes: a.rowbytes,
        range: 0,            // unused for f32 path; the second arg below carries the real value
        line_weight: a.line_weight,
        j_start: a.j_start,
        j_end: a.j_end,
        i_start: a.i_start,
        i_end: a.i_end,
        parallel: a.parallel,
    };
    run_row_range::<Pixel32>(&shared, a.range);
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
unsafe fn run_row_range<P: SmoothPixel>(a: &RowRangeArgs, range: P::Scalar) {
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

// ============================================================================
// Phase 2-A.3 Sub-stage C-2: GPU plumbing FFI
// ============================================================================
//
// All functions here are backend-neutral — the C++ Effect.cpp surface uses
// them for sequence_data UUID lifecycle, per-instance fallen state, plugin-
// global backend health, and dev-only fault injection. The actual Metal /
// CUDA dispatch lives in `gpu/metal.rs` / `gpu/cuda.rs` and is not exposed
// to C from this layer (Sub-stage C-2.5 / E will route through opaque
// FrameContext handles instead, keeping the C surface minimal).

/// Generate a fresh UUID v4 and return it split into two u64 halves so the
/// C++ side can hand it across the FFI without depending on a uuid type.
/// Convention: `out_lo` = low 64 bits, `out_hi` = high 64 bits, both little-
/// endian as Rust native. The matching `(lo, hi) -> u128` reconstruction is
/// `((hi as u128) << 64) | (lo as u128)`.
///
/// SAFETY: caller must pass valid pointers to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn smooth_core_gpu_uuid_new(out_lo: *mut u64, out_hi: *mut u64) {
    let id = uuid::Uuid::new_v4().as_u128();
    *out_lo = id as u64;
    *out_hi = (id >> 64) as u64;
}

#[inline]
fn uuid_from_halves(lo: u64, hi: u64) -> u128 {
    ((hi as u128) << 64) | (lo as u128)
}

/// Mark the per-instance UUID as having had at least one GPU failure during
/// the current SETUP/RESETUP span. Called from Effect.cpp's GPU error handler
/// (after the device→host→device fallback in §4.4 採用 (i)).
#[no_mangle]
pub extern "C" fn smooth_core_gpu_mark_fallen(uuid_lo: u64, uuid_hi: u64) {
    gpu::fallback::mark_fallen(uuid_from_halves(uuid_lo, uuid_hi));
}

/// Read the fallen flag for a UUID. Effect.cpp's SmartPreRender uses this in
/// the 5-condition AND that gates `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE`.
/// Returns 1 if fallen, 0 otherwise.
#[no_mangle]
pub extern "C" fn smooth_core_gpu_is_fallen(uuid_lo: u64, uuid_hi: u64) -> i32 {
    if gpu::fallback::is_fallen(uuid_from_halves(uuid_lo, uuid_hi)) { 1 } else { 0 }
}

/// Drop the fallen entry for a UUID. Called from `PF_Cmd_SEQUENCE_SETDOWN` so
/// project reopen (or any path that produces a new UUID) starts with a clean
/// state.
#[no_mangle]
pub extern "C" fn smooth_core_gpu_forget(uuid_lo: u64, uuid_hi: u64) {
    gpu::fallback::forget(uuid_from_halves(uuid_lo, uuid_hi));
}

/// Plugin-global backend health flag, set once at `PF_Cmd_GLOBAL_SETUP`
/// (Sub-stage D will wire `set_backend_usable` to the §4.3 detection result).
/// `usable` is treated as a boolean: 0 = false, anything else = true.
#[no_mangle]
pub extern "C" fn smooth_core_gpu_set_backend_usable(usable: i32) {
    gpu::detection::set_backend_usable(usable != 0);
}

#[no_mangle]
pub extern "C" fn smooth_core_gpu_is_backend_usable() -> i32 {
    if gpu::detection::is_backend_usable() { 1 } else { 0 }
}

/// Dev-only fault injection. Reads env var `SMOOTH_FORCE_GPU_ERROR` and
/// returns 1 if it equals the requested point, 0 otherwise. Used by
/// Effect.cpp / future Rust GPU code to simulate failures without rebuilding
/// the plugin. Cheap (one getenv per call); Release builds keep the call
/// because the env var is normally unset and the function is a 100ns no-op.
///
/// `point` codes (kept as integers so the C side does not need a string
/// literal contract):
///   1 = "setup"   — fail at GPU_DEVICE_SETUP
///   2 = "render"  — fail mid SMART_RENDER_GPU
///   3 = "oom"     — simulate VRAM OOM during render allocation
#[no_mangle]
pub extern "C" fn smooth_core_gpu_should_force_error(point: i32) -> i32 {
    let want = match point {
        1 => "setup",
        2 => "render",
        3 => "oom",
        _ => return 0,
    };
    match std::env::var("SMOOTH_FORCE_GPU_ERROR") {
        Ok(v) if v == want => 1,
        _ => 0,
    }
}

// ============================================================================
// Phase 2-A.3 Sub-stage C-2.5a: Mac Metal dispatch FFI
// ============================================================================
//
// These FFI symbols are macOS-only. On Windows the staticlib does not export
// them — the Win build of Effect.cpp will live behind `#ifdef __APPLE__` for
// the Metal path and use CUDA-specific FFI (Sub-stage E) instead.
//
// Lifecycle is tied to AE's GPU_DEVICE_SETUP / SETDOWN selectors:
//   create  — called from GPU_DEVICE_SETUP after the suite returns the
//             MTLDevice / MTLCommandQueue raw pointers. The opaque handle
//             is stashed in `PF_GPUDeviceSetupOutput->gpu_data`, then AE
//             round-trips it back to us in `PF_SmartRenderInput->gpu_data`.
//   destroy — called from GPU_DEVICE_SETDOWN to release the handle.
//   dispatch_passthrough — called from SMART_RENDER_GPU; runs the existing
//             identity passthrough kernel (Sub-stage C-1) so we can verify
//             the round-trip plumbing end-to-end. The real 2-pass smooth
//             kernel arrives in C-2.5b.

#[cfg(target_os = "macos")]
mod metal_ffi {
    use super::gpu;
    use core::ffi::c_void;

    /// Wraps `MetalBackend::from_ae_device` for C callers. Returns an opaque
    /// handle (`*mut c_void`) that the C++ side stores in PF_GPUDeviceSetupOutput.
    /// On any failure (null pointers, MSL compile failure, pipeline build
    /// failure) returns null — caller must check before stashing.
    ///
    /// SAFETY: `device_ptr` and `queue_ptr` must outlive the returned handle
    /// (AE guarantees this between GPU_DEVICE_SETUP and GPU_DEVICE_SETDOWN).
    #[no_mangle]
    pub unsafe extern "C" fn smooth_core_metal_create(
        device_ptr: *mut c_void,
        queue_ptr: *mut c_void,
    ) -> *mut c_void {
        match gpu::metal::MetalBackend::from_ae_device(device_ptr, queue_ptr) {
            Ok(backend) => Box::into_raw(Box::new(backend)) as *mut c_void,
            Err(_) => core::ptr::null_mut(),
        }
    }

    /// Tear down a backend handle previously returned by
    /// `smooth_core_metal_create`. Safe to pass null (no-op).
    ///
    /// SAFETY: `handle` must originate from `smooth_core_metal_create` and
    /// must not be used again after this call.
    #[no_mangle]
    pub unsafe extern "C" fn smooth_core_metal_destroy(handle: *mut c_void) {
        if !handle.is_null() {
            drop(Box::from_raw(handle as *mut gpu::metal::MetalBackend));
        }
    }

    /// Run the identity passthrough Metal kernel from
    /// `gpu/shaders/smooth.metal`. The pitches are in **pixels** (= rowbytes
    /// / 16 for the GPU's BGRA128 format). Returns 0 on success, non-zero on
    /// failure (caller marks the instance fallen and returns PF_Err_NONE per
    /// RFC §4.4 採用 (i)).
    ///
    /// SAFETY: `handle` must be a live MetalBackend pointer; `src_buf` /
    /// `dst_buf` must be MTLBuffer raw pointers obtained from AE's GPU
    /// suite for the matching device. Width/height bounded by the AE-
    /// allocated GPU world dimensions.
    #[no_mangle]
    pub unsafe extern "C" fn smooth_core_metal_dispatch_passthrough(
        handle: *mut c_void,
        src_buf: *mut c_void,
        dst_buf: *mut c_void,
        src_pitch_pixels: u32,
        dst_pitch_pixels: u32,
        width: u32,
        height: u32,
    ) -> i32 {
        if handle.is_null() { return -1; }
        let backend = &*(handle as *const gpu::metal::MetalBackend);
        let mut ctx = match <gpu::metal::MetalBackend as gpu::GpuBackend>::begin_frame(backend) {
            Ok(c) => c,
            Err(_) => return -2,
        };
        let dispatch = backend.dispatch_passthrough(
            &mut ctx, src_buf, dst_buf,
            src_pitch_pixels, dst_pitch_pixels, width, height,
        );
        if dispatch.is_err() {
            let _ = <gpu::metal::MetalBackend as gpu::GpuBackend>::finish_frame(backend, ctx);
            return -3;
        }
        if <gpu::metal::MetalBackend as gpu::GpuBackend>::finish_frame(backend, ctx).is_err() {
            return -4;
        }
        0
    }

    /// Run the full GPU smooth chain: priority_init (zero-fill the two
    /// AE-allocated priority buffers to UINT32_MAX) → smooth_combined
    /// (preprocess + detect + blend per pixel; mode_flg=15 centre
    /// averaging only — other modes are identity copy through). Returns
    /// 0 on success; non-zero return is the same opaque "kernel did not
    /// make it onto the queue" signal as the simpler dispatchers. Caller
    /// marks the instance fallen on non-zero per RFC §4.4 採用 (i).
    ///
    /// `priority_v_buf` / `priority_h_buf` are AE-allocated MTLBuffers
    /// (gpu_suite->AllocateDeviceMemory) of at least `width*height*4`
    /// bytes each. They MUST be non-null. The caller frees them via
    /// gpu_suite->FreeDeviceMemory after this call returns. They are
    /// initialised by the priority_init kernel here; prep2b.3+ kernels
    /// will consume them for line-blend write-conflict resolution.
    ///
    /// SAFETY: same as dispatch_passthrough — `handle` is a live
    /// MetalBackend, src/dst/priority_* are MTLBuffer pointers from AE's
    /// GPU suite, pitches are in pixels (= rowbytes/16 for BGRA128).
    #[no_mangle]
    pub unsafe extern "C" fn smooth_core_metal_dispatch_smooth_chain(
        handle: *mut c_void,
        src_buf: *mut c_void,
        dst_buf: *mut c_void,
        priority_v_buf: *mut c_void,
        priority_h_buf: *mut c_void,
        src_pitch_pixels: u32,
        dst_pitch_pixels: u32,
        width: u32,
        height: u32,
        logical_width: u32,
        range_f32: f32,
        white_opt: u32,
        line_weight: f32,
    ) -> i32 {
        if handle.is_null() { return -1; }
        let backend = &*(handle as *const gpu::metal::MetalBackend);
        let mut ctx = match <gpu::metal::MetalBackend as gpu::GpuBackend>::begin_frame(backend) {
            Ok(c) => c,
            Err(_) => return -2,
        };
        let dispatch = backend.dispatch_smooth_chain(
            &mut ctx, src_buf, dst_buf,
            priority_v_buf, priority_h_buf,
            src_pitch_pixels, dst_pitch_pixels,
            width, height, logical_width,
            range_f32, white_opt, line_weight,
        );
        if dispatch.is_err() {
            let _ = <gpu::metal::MetalBackend as gpu::GpuBackend>::finish_frame(backend, ctx);
            return -3;
        }
        if <gpu::metal::MetalBackend as gpu::GpuBackend>::finish_frame(backend, ctx).is_err() {
            return -4;
        }
        0
    }

    /// Run the preprocess kernel: copy src → dst with optional white-key
    /// stripping. Mirrors the in-place stripping half of `pre_process` in
    /// `preprocess.rs`. `white_opt` is 0/1 — 1 strips RGB=(1,1,1) pixels to
    /// the null pixel; 0 degenerates to a copy. Same return-code shape as
    /// `dispatch_passthrough`. Pitches in pixels, BGRA128 layout.
    #[no_mangle]
    pub unsafe extern "C" fn smooth_core_metal_dispatch_preprocess(
        handle: *mut c_void,
        src_buf: *mut c_void,
        dst_buf: *mut c_void,
        src_pitch_pixels: u32,
        dst_pitch_pixels: u32,
        width: u32,
        height: u32,
        white_opt: u32,
    ) -> i32 {
        if handle.is_null() { return -1; }
        let backend = &*(handle as *const gpu::metal::MetalBackend);
        let mut ctx = match <gpu::metal::MetalBackend as gpu::GpuBackend>::begin_frame(backend) {
            Ok(c) => c,
            Err(_) => return -2,
        };
        let dispatch = backend.dispatch_preprocess(
            &mut ctx, src_buf, dst_buf,
            src_pitch_pixels, dst_pitch_pixels, width, height, white_opt,
        );
        if dispatch.is_err() {
            let _ = <gpu::metal::MetalBackend as gpu::GpuBackend>::finish_frame(backend, ctx);
            return -3;
        }
        if <gpu::metal::MetalBackend as gpu::GpuBackend>::finish_frame(backend, ctx).is_err() {
            return -4;
        }
        0
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn create_with_null_returns_null() {
            unsafe {
                assert!(smooth_core_metal_create(core::ptr::null_mut(), core::ptr::null_mut())
                    .is_null());
            }
        }

        #[test]
        fn destroy_null_is_safe() {
            unsafe { smooth_core_metal_destroy(core::ptr::null_mut()); }
        }

        #[test]
        fn dispatch_with_null_handle_returns_error() {
            unsafe {
                let rc = smooth_core_metal_dispatch_passthrough(
                    core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(),
                    0, 0, 0, 0,
                );
                assert_eq!(rc, -1);
            }
        }
    }
}

#[cfg(test)]
mod gpu_ffi_tests {
    use super::*;

    #[test]
    fn uuid_round_trip() {
        let mut lo: u64 = 0;
        let mut hi: u64 = 0;
        unsafe { smooth_core_gpu_uuid_new(&mut lo, &mut hi); }
        let id = uuid_from_halves(lo, hi);
        assert_ne!(id, 0);
        // v4 sets the version nibble in the high half (bits 76..79 = 0x4).
        // Our halves layout: hi = bits 64..127, so version is in hi bits 12..15.
        let version = (hi >> 12) & 0xF;
        assert_eq!(version, 4, "uuid v4 version nibble");
    }

    #[test]
    fn fallen_lifecycle_via_ffi() {
        let mut lo: u64 = 0;
        let mut hi: u64 = 0;
        unsafe { smooth_core_gpu_uuid_new(&mut lo, &mut hi); }
        assert_eq!(smooth_core_gpu_is_fallen(lo, hi), 0);
        smooth_core_gpu_mark_fallen(lo, hi);
        assert_eq!(smooth_core_gpu_is_fallen(lo, hi), 1);
        smooth_core_gpu_forget(lo, hi);
        assert_eq!(smooth_core_gpu_is_fallen(lo, hi), 0);
    }

    #[test]
    fn backend_usable_toggle_via_ffi() {
        let prev = smooth_core_gpu_is_backend_usable();
        smooth_core_gpu_set_backend_usable(1);
        assert_eq!(smooth_core_gpu_is_backend_usable(), 1);
        smooth_core_gpu_set_backend_usable(0);
        assert_eq!(smooth_core_gpu_is_backend_usable(), 0);
        smooth_core_gpu_set_backend_usable(prev);
    }

    #[test]
    fn force_error_unset_returns_zero() {
        // Test isolation: only assert when env var is absent. The harness may
        // legitimately set it for a different test run.
        if std::env::var_os("SMOOTH_FORCE_GPU_ERROR").is_none() {
            assert_eq!(smooth_core_gpu_should_force_error(1), 0);
            assert_eq!(smooth_core_gpu_should_force_error(2), 0);
            assert_eq!(smooth_core_gpu_should_force_error(3), 0);
        }
    }
}
