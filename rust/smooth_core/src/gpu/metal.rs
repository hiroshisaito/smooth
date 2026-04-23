// Mac Metal backend — stub for Sub-stage B. Real `metal-rs` / `objc2-metal`
// bindings, MSL library load, and compute pipeline wiring land in Sub-stage C.
//
// Compiled only on macOS. Present here so the module tree, `GpuBackend` trait
// impl, and dispatch glue are in place before Sub-stage C's Metal code arrives.

use super::{FrameContext, GpuBackend, GpuError};

pub struct MetalBackend {
    // Placeholder; Sub-stage C will add: device, command_queue (AE-provided,
    // non-owning), pipeline (built in GPU_DEVICE_SETUP from embedded MSL).
}

impl MetalBackend {
    /// Stubbed out — Sub-stage C wires this to AE's `PF_GPUDeviceInfo` via
    /// FFI from Effect.cpp's GPU_DEVICE_SETUP handler.
    pub fn from_ae_device(_device_ptr: *mut std::ffi::c_void,
                          _queue_ptr: *mut std::ffi::c_void)
        -> Result<Self, GpuError>
    {
        Err(GpuError::NotAvailable)
    }
}

impl GpuBackend for MetalBackend {
    fn begin_frame(&self) -> Result<FrameContext, GpuError> {
        Err(GpuError::NotAvailable)
    }
    fn finish_frame(&self, _ctx: FrameContext) -> Result<(), GpuError> {
        Err(GpuError::NotAvailable)
    }
    fn name(&self) -> &'static str { "metal" }
}
