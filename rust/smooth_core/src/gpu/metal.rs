// Mac Metal backend (Sub-stage C-1: plumbing only — identity dispatch).
//
// Real 2-pass smooth (detect + blend) lands in Sub-stage C-2 once this
// plumbing has been validated end-to-end. The shader currently runs an
// identity pass-through so we can verify:
//   - device pointer wrap from AE's PF_GPUDeviceInfo round-trips
//   - MSL `newLibraryWithSource` compile path works
//   - compute pipeline + command buffer + commit reach the GPU and return
//   - finish_frame consumes ctx without leaking command buffers
//
// RFC §3.3.6 invariants enforced here:
//   - commandBuffer.commit only — no waitUntilCompleted (AE synchronises)
//   - per-call command buffer / encoder lives in FrameContext, not on &self
//   - &self holds only the read-only library + pipeline (built at SETUP)

use super::{FrameContext, GpuBackend, GpuError};

use std::ffi::c_void;

use metal::{
    Buffer, CommandBufferRef, CommandQueue, ComputePipelineState, Device, Library,
    MTLResourceOptions, MTLSize,
};
use objc::rc::autoreleasepool;

#[inline]
fn build_pipeline(
    device: &Device,
    library: &Library,
    fn_name: &str,
) -> Result<ComputePipelineState, GpuError> {
    let function = library
        .get_function(fn_name, None)
        .map_err(|e| GpuError::DeviceSetup(format!("get fn {fn_name}: {e}")))?;
    device
        .new_compute_pipeline_state_with_function(&function)
        .map_err(|e| GpuError::DeviceSetup(format!("pipeline {fn_name}: {e}")))
}

/// Inline MSL source for the Sub-stage C-1 identity kernel. Sub-stage C-2
/// replaces this with the 2-pass detect + blend implementation.
const SMOOTH_MSL: &str = include_str!("shaders/smooth.metal");

/// Per-device read-only state. Created once in `from_ae_device()` (called
/// from Effect.cpp's GPU_DEVICE_SETUP), reused for the device's lifetime.
pub struct MetalBackend {
    device: Device,
    queue: CommandQueue,
    pipeline_passthrough: ComputePipelineState,
    /// Sub-stage C-2.5b.1: preprocess (white-key strip + copy).
    /// Replaces passthrough as the production-path entry kernel.
    pipeline_preprocess: ComputePipelineState,
    /// Sub-stage C-2.5b.2-prep1: detect (write per-pixel mode_flg byte to
    /// an intermediate buffer for the blend pass to consume).
    pipeline_detect: ComputePipelineState,
    /// Sub-stage C-2.5b.2-prep2a: blend (mode_flg=15 centre pixel only;
    /// other modes / line-level blends arrive in subsequent preps).
    /// Kept for potential future multi-pass scenarios; the production
    /// path now uses pipeline_combined to avoid intermediate buffers.
    pipeline_blend: ComputePipelineState,
    /// Sub-stage C-2.5b.2-prep2a follow-up: combined kernel that does
    /// preprocess+detect+blend per-pixel without intermediates. This is
    /// the production path — chain-style dispatchers still exist but
    /// were observed to occasionally trip AE's "smooth did not render
    /// anything" warning under MFR + 4K + memory pressure.
    pipeline_combined: ComputePipelineState,
    /// Sub-stage C-2.5b.2-prep2b.2a: priority-buffer init kernel that
    /// fills the two `width × height × uint32` AE-allocated priority
    /// buffers with UINT32_MAX. Required before the line-blend kernels
    /// (claim/apply) so atomic_min reduces to "lowest source-i-index
    /// that touched this pixel" without a separate "untouched"
    /// sentinel.
    pipeline_priority_init: ComputePipelineState,
    /// Sub-stage C-2.5b.2-prep2b.2b: claim phase for mode_flg=15 outside
    /// line-blends, dispatched per-tile (see TILE_SIZE in
    /// dispatch_smooth_chain). Each thread = candidate centre within
    /// the tile.
    pipeline_blend_mode15_outside_claim: ComputePipelineState,
    /// Sub-stage C-2.5b.2-prep2b.2b: apply phase for mode_flg=15 outside
    /// line-blends, also tiled. Mirror of claim — reads priority and
    /// conditionally writes the blend.
    pipeline_blend_mode15_outside_apply: ComputePipelineState,
}

/// Tile size for prep2b.2b claim/apply dispatches. Each tile is encoded
/// as one dispatch_thread_groups call within the same compute encoder
/// (and hence the same command buffer), so atomic_min semantics on the
/// priority buffers are preserved across tiles.
///
/// Sized to keep per-dispatch GPU runtime well under the macOS Metal
/// driver watchdog (~2s/dispatch). At 4400² with ~10% mode_flg=15
/// density, a 512×512 tile has ≈26K candidate centres, each doing up
/// to ~1024 ops in claim or apply. ≈26M ops/tile → ~1ms on Apple
/// silicon. Total tile count for 4400×4400 = ⌈4400/512⌉² = 81. Total
/// claim+apply runtime ≈ 162ms (well within AE per-frame budget).
const TILE_SIZE: u32 = 512;

unsafe impl Send for MetalBackend {}
unsafe impl Sync for MetalBackend {}

impl MetalBackend {
    /// Wrap AE-provided device + command queue raw pointers. Both pointers
    /// are owned by AE; we hold non-owning references via metal-rs's
    /// `Device` / `CommandQueue` (which use Objective-C ARC to retain the
    /// underlying objects for our lifetime).
    ///
    /// # Safety
    /// `device_ptr` must point to a valid `id<MTLDevice>` for the lifetime
    /// of the returned `MetalBackend`. `queue_ptr` likewise for
    /// `id<MTLCommandQueue>`. AE's contract guarantees this for the span
    /// between `GPU_DEVICE_SETUP` and `GPU_DEVICE_SETDOWN`.
    pub unsafe fn from_ae_device(
        device_ptr: *mut c_void,
        queue_ptr: *mut c_void,
    ) -> Result<Self, GpuError> {
        if device_ptr.is_null() || queue_ptr.is_null() {
            return Err(GpuError::NotAvailable);
        }
        // metal-rs's `Device` / `CommandQueue` wrap the raw Objective-C ids
        // and retain them on construction.
        let device: Device =
            std::mem::transmute::<*mut c_void, &metal::DeviceRef>(device_ptr).to_owned();
        let queue: CommandQueue =
            std::mem::transmute::<*mut c_void, &metal::CommandQueueRef>(queue_ptr).to_owned();

        autoreleasepool(|| {
            let library: Library = device
                .new_library_with_source(SMOOTH_MSL, &metal::CompileOptions::new())
                .map_err(|e| GpuError::DeviceSetup(format!("MSL compile: {e}")))?;
            let pipeline_passthrough    = build_pipeline(&device, &library, "smooth_passthrough")?;
            let pipeline_preprocess     = build_pipeline(&device, &library, "smooth_preprocess")?;
            let pipeline_detect         = build_pipeline(&device, &library, "smooth_detect")?;
            let pipeline_blend          = build_pipeline(&device, &library, "smooth_blend")?;
            let pipeline_combined       = build_pipeline(&device, &library, "smooth_combined")?;
            let pipeline_priority_init  = build_pipeline(&device, &library, "smooth_priority_init")?;
            let pipeline_blend_mode15_outside_claim = build_pipeline(&device, &library, "smooth_blend_mode15_outside_claim")?;
            let pipeline_blend_mode15_outside_apply = build_pipeline(&device, &library, "smooth_blend_mode15_outside_apply")?;

            Ok(MetalBackend {
                device,
                queue,
                pipeline_passthrough,
                pipeline_preprocess,
                pipeline_detect,
                pipeline_blend,
                pipeline_combined,
                pipeline_priority_init,
                pipeline_blend_mode15_outside_claim,
                pipeline_blend_mode15_outside_apply,
            })
        })
    }

    /// Construct a MetalBackend from the system default device. Test-only
    /// path so unit tests can run on the development host without AE.
    #[cfg(test)]
    pub fn for_test() -> Result<Self, GpuError> {
        let device = Device::system_default().ok_or(GpuError::NotAvailable)?;
        let queue = device.new_command_queue();
        autoreleasepool(|| {
            let library = device
                .new_library_with_source(SMOOTH_MSL, &metal::CompileOptions::new())
                .map_err(|e| GpuError::DeviceSetup(format!("MSL compile: {e}")))?;
            let pipeline_passthrough    = build_pipeline(&device, &library, "smooth_passthrough")?;
            let pipeline_preprocess     = build_pipeline(&device, &library, "smooth_preprocess")?;
            let pipeline_detect         = build_pipeline(&device, &library, "smooth_detect")?;
            let pipeline_blend          = build_pipeline(&device, &library, "smooth_blend")?;
            let pipeline_combined       = build_pipeline(&device, &library, "smooth_combined")?;
            let pipeline_priority_init  = build_pipeline(&device, &library, "smooth_priority_init")?;
            let pipeline_blend_mode15_outside_claim = build_pipeline(&device, &library, "smooth_blend_mode15_outside_claim")?;
            let pipeline_blend_mode15_outside_apply = build_pipeline(&device, &library, "smooth_blend_mode15_outside_apply")?;
            Ok(MetalBackend {
                device,
                queue,
                pipeline_passthrough,
                pipeline_preprocess,
                pipeline_detect,
                pipeline_blend,
                pipeline_combined,
                pipeline_priority_init,
                pipeline_blend_mode15_outside_claim,
                pipeline_blend_mode15_outside_apply,
            })
        })
    }

    /// Sub-stage C-2.5b.2-prep1: smooth_detect. Writes a per-pixel
    /// `mode_flg` byte to a freshly-allocated MTLBuffer of size
    /// `width * height` bytes; the buffer is returned so the caller can
    /// pass it to the blend kernel (or, in current code, drop it after
    /// inspection). The `src` buffer is the GPU input world (BGRA128);
    /// `src_pitch_pixels` and `width`/`height`/`logical_width` mirror
    /// what `process_row_range` would see on the CPU side.
    ///
    /// Returns the modes buffer on success so unit tests / future blend
    /// dispatches can consume it. Caller drops the returned `Buffer` to
    /// release.
    pub fn dispatch_detect(
        &self,
        ctx: &mut FrameContext,
        src_buffer_ptr: *mut c_void,
        src_pitch_pixels: u32,
        width: u32,
        height: u32,
        logical_width: u32,
        range: f32,
    ) -> Result<Buffer, GpuError> {
        if src_buffer_ptr.is_null() {
            return Err(GpuError::Dispatch("null src buffer".into()));
        }
        if width == 0 || height == 0 {
            return Err(GpuError::Dispatch("zero extent".into()));
        }
        let _ = &ctx.scratch;
        let modes_bytes = (width as u64) * (height as u64);
        let modes_buf = self.device.new_buffer(
            modes_bytes,
            MTLResourceOptions::StorageModeShared,
        );

        autoreleasepool(|| -> Result<(), GpuError> {
            let cb: &CommandBufferRef = self.queue.new_command_buffer();
            let enc = cb.new_compute_command_encoder();
            enc.set_compute_pipeline_state(&self.pipeline_detect);

            let src = unsafe {
                std::mem::transmute::<*mut c_void, &metal::BufferRef>(src_buffer_ptr)
            };
            enc.set_buffer(0, Some(src), 0);
            enc.set_buffer(1, Some(&modes_buf), 0);
            enc.set_bytes(2, 4, &src_pitch_pixels as *const u32 as *const c_void);
            enc.set_bytes(3, 4, &width  as *const u32 as *const c_void);
            enc.set_bytes(4, 4, &height as *const u32 as *const c_void);
            enc.set_bytes(5, 4, &logical_width as *const u32 as *const c_void);
            enc.set_bytes(6, 4, &range as *const f32 as *const c_void);

            let group = MTLSize::new(16, 16, 1);
            let groups = MTLSize::new(
                ((width + 15) / 16) as u64,
                ((height + 15) / 16) as u64,
                1,
            );
            enc.dispatch_thread_groups(groups, group);
            enc.end_encoding();
            cb.commit();
            // Wait for completion so callers (unit tests / future blend
            // chain) can read the modes buffer back. Sub-stage C-2.5b.2-
            // prep2's blend pass will replace this with a serial commit
            // that lets Metal pipeline the two passes.
            cb.wait_until_completed();
            Ok(())
        })?;

        Ok(modes_buf)
    }

    /// Sub-stage C-2.5b.1: preprocess. Mirrors `pre_process` in
    /// `preprocess.rs` for the white-key stripping half. `white_opt` is a
    /// boolean encoded as 0/1 (matches `info->white_option ? 1 : 0` on the
    /// C++ side; matches the kernel's `constant uint& white_opt`). Pitches
    /// in pixels per the same convention as `dispatch_passthrough`.
    pub fn dispatch_preprocess(
        &self,
        ctx: &mut FrameContext,
        src_buffer_ptr: *mut c_void,
        dst_buffer_ptr: *mut c_void,
        src_pitch_pixels: u32,
        dst_pitch_pixels: u32,
        width: u32,
        height: u32,
        white_opt: u32,
    ) -> Result<(), GpuError> {
        if src_buffer_ptr.is_null() || dst_buffer_ptr.is_null() {
            return Err(GpuError::Dispatch("null buffer".into()));
        }
        if width == 0 || height == 0 {
            return Err(GpuError::Dispatch("zero extent".into()));
        }
        let _ = &ctx.scratch;
        autoreleasepool(|| -> Result<(), GpuError> {
            let cb: &CommandBufferRef = self.queue.new_command_buffer();
            let enc = cb.new_compute_command_encoder();
            enc.set_compute_pipeline_state(&self.pipeline_preprocess);
            let src = unsafe {
                std::mem::transmute::<*mut c_void, &metal::BufferRef>(src_buffer_ptr)
            };
            let dst = unsafe {
                std::mem::transmute::<*mut c_void, &metal::BufferRef>(dst_buffer_ptr)
            };
            enc.set_buffer(0, Some(src), 0);
            enc.set_buffer(1, Some(dst), 0);
            enc.set_bytes(2, 4, &src_pitch_pixels as *const u32 as *const c_void);
            enc.set_bytes(3, 4, &dst_pitch_pixels as *const u32 as *const c_void);
            enc.set_bytes(4, 4, &width  as *const u32 as *const c_void);
            enc.set_bytes(5, 4, &height as *const u32 as *const c_void);
            enc.set_bytes(6, 4, &white_opt as *const u32 as *const c_void);
            let group = MTLSize::new(16, 16, 1);
            let groups = MTLSize::new(
                ((width + 15) / 16) as u64,
                ((height + 15) / 16) as u64,
                1,
            );
            enc.dispatch_thread_groups(groups, group);
            enc.end_encoding();
            cb.commit();
            Ok(())
        })
    }

    /// Sub-stage C-2.5b.2-prep2b.2b: smooth chain dispatcher with
    /// priority buffers + mode_flg=15 outside line-blend kernels. Runs
    /// 4 passes within a single command buffer:
    ///   1. priority_init: zero-fill priority_v / priority_h to
    ///      UINT32_MAX.
    ///   2. smooth_combined: preprocess + detect + mode_flg=15 inside
    ///      (centre 4-corner avg). All other mode_flg values pass
    ///      through from src.
    ///   3. blend_mode15_outside_claim (TILED): each candidate centre
    ///      with mode_flg=15 walks its 4 outside-line calls and
    ///      atomic_min on the touched output pixels' priority slots.
    ///      Encoded as N×M dispatch_thread_groups calls, one per
    ///      TILE_SIZE×TILE_SIZE tile, to keep per-dispatch GPU runtime
    ///      under the macOS Metal driver watchdog.
    ///   4. blend_mode15_outside_apply (TILED): same per-tile structure
    ///      as claim; reads priority and conditionally writes blend.
    ///
    /// 2026-05-04: an earlier non-tiled implementation (commit ac408f7)
    /// dispatched 19M threads per kernel and triggered AE warning
    /// "smooth did not render anything" + FrameTask 517 errors at 4400²
    /// resolution — symptoms of GPU watchdog timeout on heavy
    /// mode_flg=15-dense regions. That commit was reverted; this tiled
    /// version (commit prep2b.2b retry) keeps each dispatch's workload
    /// bounded.
    ///
    /// `priority_v` / `priority_h` are AE-allocated MTLBuffers (see
    /// gpu_suite->AllocateDeviceMemory in Effect.cpp). They MUST be
    /// non-null and at least `width * height * 4` bytes; the caller
    /// owns them and frees via gpu_suite->FreeDeviceMemory after the
    /// dispatch.
    ///
    /// `line_weight` is the per-blend line weighting (CPU-side encoding
    /// `(slider_value / 2.0 + 0.5)`) used by the outside-line blend
    /// kernels.
    ///
    /// Blend coverage:
    ///   - mode_flg = 15 inside: pipeline_combined, centre 4-corner avg.
    ///   - mode_flg = 15 outside: pipelines_blend_mode15_outside_*
    ///     handle line-blends via atomic_min priority resolution
    ///     ("lowest source-i_index wins").
    ///   - mode_flg ∈ {3, 5, 7, 11, 13}: still identity copy from src
    ///     (added in prep2b.3+).
    pub fn dispatch_smooth_chain(
        &self,
        ctx: &mut FrameContext,
        src_buf:           *mut c_void,
        dst_buf:           *mut c_void,
        priority_v_buf:    *mut c_void,
        priority_h_buf:    *mut c_void,
        src_pitch_pixels:  u32,
        dst_pitch_pixels:  u32,
        width:             u32,
        height:            u32,
        logical_width:     u32,
        range_f32:         f32,
        white_opt:         u32,
        line_weight:       f32,
    ) -> Result<(), GpuError> {
        if src_buf.is_null() || dst_buf.is_null() {
            return Err(GpuError::Dispatch("null src/dst buffer".into()));
        }
        if priority_v_buf.is_null() || priority_h_buf.is_null() {
            return Err(GpuError::Dispatch("null priority buffer".into()));
        }
        if width == 0 || height == 0 {
            return Err(GpuError::Dispatch("zero extent".into()));
        }
        let _ = &ctx.scratch;

        autoreleasepool(|| -> Result<(), GpuError> {
            let cb: &CommandBufferRef = self.queue.new_command_buffer();

            let src = unsafe {
                std::mem::transmute::<*mut c_void, &metal::BufferRef>(src_buf)
            };
            let dst = unsafe {
                std::mem::transmute::<*mut c_void, &metal::BufferRef>(dst_buf)
            };
            let pri_v = unsafe {
                std::mem::transmute::<*mut c_void, &metal::BufferRef>(priority_v_buf)
            };
            let pri_h = unsafe {
                std::mem::transmute::<*mut c_void, &metal::BufferRef>(priority_h_buf)
            };

            let group = MTLSize::new(16, 16, 1);
            let groups = MTLSize::new(
                ((width + 15) / 16) as u64,
                ((height + 15) / 16) as u64,
                1,
            );

            // Pass 1: priority_init — zero-fill priority_v / priority_h to
            // UINT32_MAX. Encoded first so the combined kernel (and future
            // claim/apply kernels) see initialised buffers.
            {
                let enc = cb.new_compute_command_encoder();
                enc.set_compute_pipeline_state(&self.pipeline_priority_init);
                enc.set_buffer(0, Some(pri_v), 0);
                enc.set_buffer(1, Some(pri_h), 0);
                enc.set_bytes(2, 4, &width  as *const u32 as *const c_void);
                enc.set_bytes(3, 4, &height as *const u32 as *const c_void);
                enc.dispatch_thread_groups(groups, group);
                enc.end_encoding();
            }

            // Pass 2: combined — preprocess + detect + mode_flg=15
            // inside (centre 4-corner avg). Priority buffers not bound
            // here; consumed by passes 3+4 below.
            {
                let enc = cb.new_compute_command_encoder();
                enc.set_compute_pipeline_state(&self.pipeline_combined);
                enc.set_buffer(0, Some(src), 0);
                enc.set_buffer(1, Some(dst), 0);
                enc.set_bytes(2, 4, &src_pitch_pixels as *const u32 as *const c_void);
                enc.set_bytes(3, 4, &dst_pitch_pixels as *const u32 as *const c_void);
                enc.set_bytes(4, 4, &width  as *const u32 as *const c_void);
                enc.set_bytes(5, 4, &height as *const u32 as *const c_void);
                enc.set_bytes(6, 4, &logical_width as *const u32 as *const c_void);
                enc.set_bytes(7, 4, &range_f32 as *const f32 as *const c_void);
                enc.set_bytes(8, 4, &white_opt as *const u32 as *const c_void);
                enc.dispatch_thread_groups(groups, group);
                enc.end_encoding();
            }

            // Helper to encode the per-tile dispatches for either claim
            // or apply pass. Each tile is one dispatch_thread_groups
            // call; all tiles share a single compute encoder so atomic
            // semantics across tiles are preserved (Metal serialises
            // dispatches within a single encoder + command buffer).
            //
            // Tile loop: for tile_y in 0..height step TILE_SIZE
            //              for tile_x in 0..width  step TILE_SIZE
            //                  set_bytes(tile_origin) + dispatch
            let encode_tiled_pass = |pipeline: &ComputePipelineState| {
                let enc = cb.new_compute_command_encoder();
                enc.set_compute_pipeline_state(pipeline);
                // Bindings 0..11 are constant across tiles; encode once.
                enc.set_buffer(0, Some(src), 0);
                enc.set_buffer(1, Some(dst), 0);
                enc.set_buffer(2, Some(pri_v), 0);
                enc.set_buffer(3, Some(pri_h), 0);
                enc.set_bytes(4, 4, &src_pitch_pixels as *const u32 as *const c_void);
                enc.set_bytes(5, 4, &dst_pitch_pixels as *const u32 as *const c_void);
                enc.set_bytes(6, 4, &width  as *const u32 as *const c_void);
                enc.set_bytes(7, 4, &height as *const u32 as *const c_void);
                enc.set_bytes(8, 4, &logical_width as *const u32 as *const c_void);
                enc.set_bytes(9, 4, &range_f32 as *const f32 as *const c_void);
                enc.set_bytes(10, 4, &white_opt as *const u32 as *const c_void);
                enc.set_bytes(11, 4, &line_weight as *const f32 as *const c_void);

                let mut tile_y: u32 = 0;
                while tile_y < height {
                    let mut tile_x: u32 = 0;
                    while tile_x < width {
                        let tw = (TILE_SIZE).min(width  - tile_x);
                        let th = (TILE_SIZE).min(height - tile_y);
                        let tile_origin: [u32; 2] = [tile_x, tile_y];
                        enc.set_bytes(12, 8, tile_origin.as_ptr() as *const c_void);
                        let tile_groups = MTLSize::new(
                            ((tw + 15) / 16) as u64,
                            ((th + 15) / 16) as u64,
                            1,
                        );
                        enc.dispatch_thread_groups(tile_groups, group);
                        tile_x += TILE_SIZE;
                    }
                    tile_y += TILE_SIZE;
                }
                enc.end_encoding();
            };

            // Pass 3: claim — atomic_min on touched output pixels'
            // priority slots. No dst writes.
            encode_tiled_pass(&self.pipeline_blend_mode15_outside_claim);
            // Pass 4: apply — read priority + conditional dst writes
            // for centres that won the claim.
            encode_tiled_pass(&self.pipeline_blend_mode15_outside_apply);

            cb.commit();
            // RFC §3.3.6 contract: no Rust-allocated intermediates. The
            // priority buffers are AE-allocated (gpu_suite) and live for
            // the whole call → AE's synchroniser sees them.
            Ok(())
        })
    }

    /// Sub-stage C-1 dispatch: identity passthrough. Both buffers must hold
    /// `width * height` BGRA128 (4×f32) pixels. `src_pitch_pixels` /
    /// `dst_pitch_pixels` are pitches in **pixels**, not bytes (matches
    /// MSL kernel signature; matches what AE provides via
    /// `rowbytes / 16`).
    ///
    /// Production GPU path now uses `dispatch_smooth_chain`; this remains
    /// available for unit tests and as a minimal-ops debugging probe.
    pub fn dispatch_passthrough(
        &self,
        ctx: &mut FrameContext,
        src_buffer_ptr: *mut c_void,
        dst_buffer_ptr: *mut c_void,
        src_pitch_pixels: u32,
        dst_pitch_pixels: u32,
        width: u32,
        height: u32,
    ) -> Result<(), GpuError> {
        if src_buffer_ptr.is_null() || dst_buffer_ptr.is_null() {
            return Err(GpuError::Dispatch("null buffer".into()));
        }
        if width == 0 || height == 0 {
            return Err(GpuError::Dispatch("zero extent".into()));
        }
        // FrameContext is unused for now; will hold per-call MTLBuffer
        // wrappers + intermediate allocations in Sub-stage C-2.
        let _ = &ctx.scratch;

        autoreleasepool(|| -> Result<(), GpuError> {
            let cb: &CommandBufferRef = self.queue.new_command_buffer();
            let enc = cb.new_compute_command_encoder();
            enc.set_compute_pipeline_state(&self.pipeline_passthrough);

            // Bind src / dst buffers from raw pointers (AE-owned MTLBuffer).
            let src = unsafe {
                std::mem::transmute::<*mut c_void, &metal::BufferRef>(src_buffer_ptr)
            };
            let dst = unsafe {
                std::mem::transmute::<*mut c_void, &metal::BufferRef>(dst_buffer_ptr)
            };
            enc.set_buffer(0, Some(src), 0);
            enc.set_buffer(1, Some(dst), 0);
            enc.set_bytes(2, 4, &src_pitch_pixels as *const u32 as *const c_void);
            enc.set_bytes(3, 4, &dst_pitch_pixels as *const u32 as *const c_void);
            enc.set_bytes(4, 4, &width as *const u32 as *const c_void);
            enc.set_bytes(5, 4, &height as *const u32 as *const c_void);

            let group = MTLSize::new(16, 16, 1);
            let groups = MTLSize::new(
                ((width + 15) / 16) as u64,
                ((height + 15) / 16) as u64,
                1,
            );
            enc.dispatch_thread_groups(groups, group);
            enc.end_encoding();
            cb.commit();
            // RFC §3.3.6: NO waitUntilCompleted. AE handles synchronisation.

            // Surface command-buffer-level errors that surfaced before commit.
            // We can't poll status without waiting; AE will report any
            // GPU-side faults via SDK callbacks.
            Ok(())
        })
    }
}

impl GpuBackend for MetalBackend {
    fn begin_frame(&self) -> Result<FrameContext, GpuError> {
        Ok(FrameContext::default())
    }
    fn finish_frame(&self, _ctx: FrameContext) -> Result<(), GpuError> {
        // The command buffer was committed inside dispatch_*. AE owns post-
        // commit synchronisation; nothing to do here for Sub-stage C-1.
        Ok(())
    }
    fn name(&self) -> &'static str { "metal" }
}

#[cfg(test)]
mod detect_tests {
    use super::*;
    use foreign_types::ForeignType;

    /// Build a 4×4 BGRA128 buffer where (1,1) is black and the rest is white,
    /// run dispatch_detect on it twice (once with a tight range, once loose),
    /// and verify the modes buffer matches what process_row_range's
    /// per-pixel branch would have written.
    #[test]
    fn detect_marks_centre_as_edge_with_tight_range() {
        // BGRA float4 layout — pick distinct B and W where every channel
        // differs by 1.0 (delta_sum = 4.0) so a 0.1 tolerance catches them
        // and a 5.0 tolerance absorbs them.
        let w: [f32; 4] = [1.0, 1.0, 1.0, 1.0];  // BGRA all 1.0
        let b: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

        let mut pixels = vec![w; 16];
        pixels[1 * 4 + 1] = b;  // (x=1, y=1) = black

        let backend = MetalBackend::for_test().expect("Metal backend");

        // Upload pixels to a Metal buffer.
        let bytes_len = pixels.len() * 16;
        let src = backend.device.new_buffer_with_data(
            pixels.as_ptr() as *const _,
            bytes_len as u64,
            metal::MTLResourceOptions::StorageModeShared,
        );

        let mut ctx = backend.begin_frame().unwrap();

        // Tight range: delta of 4.0 must register as an edge.
        let modes = backend
            .dispatch_detect(
                &mut ctx,
                src.as_ptr() as *mut std::ffi::c_void,
                /* src_pitch_pixels */ 4,
                /* width */            4,
                /* height */           4,
                /* logical_width */    4,
                /* range */            0.1,
            )
            .expect("detect dispatch");
        let modes_slice = unsafe {
            core::slice::from_raw_parts(modes.contents() as *const u8, 16)
        };
        // Inner region only fires at (1,1) and (2,1) (since right-of-inner
        // is x+1 < logical_width = 4 → x in [1, 2], y in [1, 2]).
        // (1,1): centre=B, right=W (different), all neighbours different → 0x8F
        // (2,1): centre=W, right=W (same bytes) → fast_compare false → 0
        // (1,2): centre=W, right=W (same bytes) → 0
        // (2,2): centre=W, right=W (same bytes) → 0
        // outside inner region → 0
        assert_eq!(modes_slice[1 * 4 + 1], 0x8F,
            "(1,1) should detect right+up+down+left edges; got 0x{:02X}",
            modes_slice[1 * 4 + 1]);
        // Spot-check a few zeros.
        for &(x, y) in &[(0u32, 0u32), (3, 0), (0, 3), (3, 3), (2, 1), (1, 2)] {
            let v = modes_slice[(y * 4 + x) as usize];
            assert_eq!(v, 0,
                "({},{}) should be 0 (outside inner region or fast_compare fail); got 0x{:02X}",
                x, y, v);
        }

        backend.finish_frame(ctx).unwrap();
    }

    #[test]
    fn detect_loose_range_records_fast_match_only() {
        let w: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
        let b: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

        let mut pixels = vec![w; 16];
        pixels[1 * 4 + 1] = b;

        let backend = MetalBackend::for_test().expect("Metal backend");
        let bytes_len = pixels.len() * 16;
        let src = backend.device.new_buffer_with_data(
            pixels.as_ptr() as *const _,
            bytes_len as u64,
            metal::MTLResourceOptions::StorageModeShared,
        );
        let mut ctx = backend.begin_frame().unwrap();

        // Loose range: delta of 4.0 is below 5.0 → no edge bits set, but
        // fast_compare still fires (bytes differ) → modes = 0x80 only.
        let modes = backend
            .dispatch_detect(
                &mut ctx,
                src.as_ptr() as *mut std::ffi::c_void,
                4, 4, 4, 4,
                /* range */ 5.0,
            )
            .expect("detect dispatch");
        let modes_slice = unsafe {
            core::slice::from_raw_parts(modes.contents() as *const u8, 16)
        };
        assert_eq!(modes_slice[1 * 4 + 1], 0x80,
            "(1,1) tolerated → only fast_match bit; got 0x{:02X}",
            modes_slice[1 * 4 + 1]);

        backend.finish_frame(ctx).unwrap();
    }
}
