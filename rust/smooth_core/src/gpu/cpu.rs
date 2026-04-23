// CPU backend wrap for the GpuBackend trait.
//
// Purpose: give fallback / unit-test / benchmark code a single trait-level
// interface so Sub-stage C's Metal backend is a drop-in for the same API.
// For Sub-stage B this is intentionally a shell — the existing CPU entry
// points (`smooth_core_process_row_range_*`) remain the hot path and are
// not reached through this trait yet (that rewiring is Sub-stage C).

use super::{FrameContext, GpuBackend, GpuError};

pub struct CpuBackend;

impl CpuBackend {
    pub fn new() -> Self { Self }
}

impl Default for CpuBackend {
    fn default() -> Self { Self::new() }
}

impl GpuBackend for CpuBackend {
    fn begin_frame(&self) -> Result<FrameContext, GpuError> {
        Ok(FrameContext::default())
    }

    fn finish_frame(&self, _ctx: FrameContext) -> Result<(), GpuError> {
        // CPU backend has no deferred work to submit; ctx's Vec is dropped
        // with the function return.
        Ok(())
    }

    fn name(&self) -> &'static str { "cpu" }
}
