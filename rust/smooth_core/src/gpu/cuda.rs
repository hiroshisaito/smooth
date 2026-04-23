// Windows CUDA backend — stub for Sub-stage B. Real NVCC static-linked
// kernel launchers (SDK sample pattern, §3.3.6 / §6.4) are added in Sub-stage E.
//
// Compiled only on Windows.

use super::{FrameContext, GpuBackend, GpuError};

pub struct CudaBackend {
    // Placeholder; Sub-stage E will add: device handle, stream (AE-provided,
    // non-owning), any NVCC-built kernel function handles.
}

impl CudaBackend {
    pub fn from_ae_device(_device_ptr: *mut std::ffi::c_void,
                          _stream_ptr: *mut std::ffi::c_void)
        -> Result<Self, GpuError>
    {
        Err(GpuError::NotAvailable)
    }
}

impl GpuBackend for CudaBackend {
    fn begin_frame(&self) -> Result<FrameContext, GpuError> {
        Err(GpuError::NotAvailable)
    }
    fn finish_frame(&self, _ctx: FrameContext) -> Result<(), GpuError> {
        Err(GpuError::NotAvailable)
    }
    fn name(&self) -> &'static str { "cuda" }
}
