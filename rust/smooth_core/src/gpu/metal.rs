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
    pipeline_blend: ComputePipelineState,
}

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
            let pipeline_passthrough = build_pipeline(&device, &library, "smooth_passthrough")?;
            let pipeline_preprocess  = build_pipeline(&device, &library, "smooth_preprocess")?;
            let pipeline_detect      = build_pipeline(&device, &library, "smooth_detect")?;
            let pipeline_blend       = build_pipeline(&device, &library, "smooth_blend")?;

            Ok(MetalBackend {
                device,
                queue,
                pipeline_passthrough,
                pipeline_preprocess,
                pipeline_detect,
                pipeline_blend,
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
            let pipeline_passthrough = build_pipeline(&device, &library, "smooth_passthrough")?;
            let pipeline_preprocess  = build_pipeline(&device, &library, "smooth_preprocess")?;
            let pipeline_detect      = build_pipeline(&device, &library, "smooth_detect")?;
            let pipeline_blend       = build_pipeline(&device, &library, "smooth_blend")?;
            Ok(MetalBackend {
                device,
                queue,
                pipeline_passthrough,
                pipeline_preprocess,
                pipeline_detect,
                pipeline_blend,
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

    /// Sub-stage C-2.5b.2-prep2a: smooth chain dispatcher.
    /// preprocess(src → inter) → detect(inter → modes) → blend(inter, modes → dst)
    /// in one command buffer. Allocates the intermediate (inter) and modes
    /// buffers internally; both are released when this method returns.
    ///
    /// The blend pass currently handles only mode_flg = 15 (link8_square
    /// centre pixel). Pixels for which detect produced any other mode_flg
    /// fall through to identity copy from `inter`. Visually this means a
    /// 32bpc render with the GPU path engaged shows the white-key strip
    /// (preprocess), the corner-pixel averaging at isolated-mode pixels,
    /// and otherwise the post-preprocess image — the staircase smoothing
    /// itself arrives once the line-level blends from prep2b+ land.
    pub fn dispatch_smooth_chain(
        &self,
        ctx: &mut FrameContext,
        src_buf:           *mut c_void,
        dst_buf:           *mut c_void,
        src_pitch_pixels:  u32,
        dst_pitch_pixels:  u32,
        width:             u32,
        height:            u32,
        logical_width:     u32,
        range_f32:         f32,
        white_opt:         u32,
    ) -> Result<(), GpuError> {
        if src_buf.is_null() || dst_buf.is_null() {
            return Err(GpuError::Dispatch("null src/dst buffer".into()));
        }
        if width == 0 || height == 0 {
            return Err(GpuError::Dispatch("zero extent".into()));
        }
        let _ = &ctx.scratch;

        // Intermediate post-preprocess buffer (private storage = GPU-only).
        let inter_pitch_pixels = width;
        let inter_bytes = (width as u64) * (height as u64) * 16;
        let inter = self.device.new_buffer(
            inter_bytes,
            MTLResourceOptions::StorageModePrivate,
        );
        let modes_bytes = (width as u64) * (height as u64);
        let modes = self.device.new_buffer(
            modes_bytes,
            MTLResourceOptions::StorageModePrivate,
        );

        autoreleasepool(|| -> Result<(), GpuError> {
            let cb: &CommandBufferRef = self.queue.new_command_buffer();

            // Single compute encoder spanning all three passes — matches the
            // SDK_Invert_ProcAmp.cpp pattern (one encoder, switch pipelines
            // between dispatches). This is more efficient than three encoders
            // and avoids any subtle ordering issues that can arise when AE's
            // internal command-buffer scheduler interacts with multiple
            // sub-encoders inside one cb.
            let enc = cb.new_compute_command_encoder();

            let src = unsafe {
                std::mem::transmute::<*mut c_void, &metal::BufferRef>(src_buf)
            };
            let dst = unsafe {
                std::mem::transmute::<*mut c_void, &metal::BufferRef>(dst_buf)
            };

            let group = MTLSize::new(16, 16, 1);
            let groups = MTLSize::new(
                ((width + 15) / 16) as u64,
                ((height + 15) / 16) as u64,
                1,
            );

            // Pass 1: preprocess src → inter.
            enc.set_compute_pipeline_state(&self.pipeline_preprocess);
            enc.set_buffer(0, Some(src),    0);
            enc.set_buffer(1, Some(&inter), 0);
            enc.set_bytes(2, 4, &src_pitch_pixels    as *const u32 as *const c_void);
            enc.set_bytes(3, 4, &inter_pitch_pixels  as *const u32 as *const c_void);
            enc.set_bytes(4, 4, &width  as *const u32 as *const c_void);
            enc.set_bytes(5, 4, &height as *const u32 as *const c_void);
            enc.set_bytes(6, 4, &white_opt as *const u32 as *const c_void);
            enc.dispatch_thread_groups(groups, group);

            // Pass 2: detect inter → modes.
            enc.set_compute_pipeline_state(&self.pipeline_detect);
            enc.set_buffer(0, Some(&inter), 0);
            enc.set_buffer(1, Some(&modes), 0);
            enc.set_bytes(2, 4, &inter_pitch_pixels as *const u32 as *const c_void);
            enc.set_bytes(3, 4, &width  as *const u32 as *const c_void);
            enc.set_bytes(4, 4, &height as *const u32 as *const c_void);
            enc.set_bytes(5, 4, &logical_width as *const u32 as *const c_void);
            enc.set_bytes(6, 4, &range_f32 as *const f32 as *const c_void);
            enc.dispatch_thread_groups(groups, group);

            // Pass 3: blend (inter, modes) → dst.
            enc.set_compute_pipeline_state(&self.pipeline_blend);
            enc.set_buffer(0, Some(&inter), 0);
            enc.set_buffer(1, Some(dst),    0);
            enc.set_buffer(2, Some(&modes), 0);
            enc.set_bytes(3, 4, &inter_pitch_pixels as *const u32 as *const c_void);
            enc.set_bytes(4, 4, &dst_pitch_pixels   as *const u32 as *const c_void);
            enc.set_bytes(5, 4, &width  as *const u32 as *const c_void);
            enc.set_bytes(6, 4, &height as *const u32 as *const c_void);
            enc.set_bytes(7, 4, &logical_width as *const u32 as *const c_void);
            enc.set_bytes(8, 4, &range_f32 as *const f32 as *const c_void);
            enc.dispatch_thread_groups(groups, group);

            enc.end_encoding();
            cb.commit();
            // Sub-stage C-2.5b.2-prep2a follow-up: wait_until_completed is
            // here as a safety net, NOT because RFC §3.3.6 was wrong in
            // general — for plugins that follow the SDK_Invert_ProcAmp
            // pattern (gpu_suite->AllocateDeviceMemory for intermediates,
            // single encoder), AE's framework synchronises commit-only.
            // Our intermediate buffers are allocated via metal-rs's
            // device.new_buffer() (StorageModePrivate, NOT registered with
            // AE's gpu_suite tracker), so AE cannot see when the inter /
            // modes buffers are still in use by a queued command buffer.
            // Without this wait, a downstream AE thread can read `dst`
            // before the GPU has actually written it — which surfaces as
            // the "smooth did not render anything" warning + scattered
            // FrameTask 517 errors observed on first install of build
            // c7e164a (2026-05-04). Migrating intermediates to
            // gpu_suite->AllocateDeviceMemory in a follow-up commit will
            // let us drop this wait.
            cb.wait_until_completed();
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
