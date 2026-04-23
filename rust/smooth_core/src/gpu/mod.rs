// Phase 2-A.3 Sub-stage B scaffold. Trait shape + module tree only; no real GPU
// dispatch yet. Sub-stage C fleshes out Metal, Sub-stage E fleshes out CUDA.
//
// Design doc: docs/PHASE_2A_GPU_RFC.md §6.1, with FrameContext-based per-call
// state so `&self` holds only read-only per-device resources (§4.1 spike
// conclusion: AE serialises SMART_RENDER_GPU per instance, so any
// shared-mutable state on &self would be a latent bug, not a performance win).

#![allow(dead_code)]  // methods get wired up in Sub-stage C/E

use std::ffi::c_void;

pub mod cpu;
pub mod fallback;
pub mod detection;

#[cfg(target_os = "macos")]
pub mod metal;

#[cfg(target_os = "windows")]
pub mod cuda;

#[cfg(test)]
mod tests;

/// Per-call buffer wrapper. Concrete type is backend-specific; Sub-stage B
/// keeps it `()` for the CPU backend. Metal/CUDA backends will introduce
/// MTLBuffer / device pointer wrappers in Sub-stage C/E.
#[derive(Debug)]
pub struct Buffer {
    /// Opaque handle. For CPU: raw pointer into a host-side Vec managed by
    /// the FrameContext. For Metal/CUDA: MTLBuffer / CUdeviceptr bound to
    /// the FrameContext lifetime.
    pub handle: *mut c_void,
    pub size_bytes: usize,
}

// Buffer handles are not Send/Sync by default; backends that support
// cross-thread use will provide an explicit wrapper type in Sub-stage C.

/// Per-call working state. Dropped with `finish_frame(ctx)` — this is the
/// §4.1 (B) constraint made type-safe: by consuming `ctx` in finish_frame,
/// the trait prevents accidental reuse across frames that would reintroduce
/// cached-command-buffer semantics.
#[derive(Debug, Default)]
pub struct FrameContext {
    /// Host-side arena for CPU backend. Metal/CUDA backends will replace
    /// this field in a dedicated wrapper struct in Sub-stage C/E.
    pub scratch: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("backend not usable on this platform / configuration")]
    NotAvailable,
    #[error("device setup failed: {0}")]
    DeviceSetup(String),
    #[error("allocation failed for {0} bytes")]
    Alloc(usize),
    #[error("kernel dispatch failed: {0}")]
    Dispatch(String),
    #[error("submit / finish failed: {0}")]
    Submit(String),
}

/// The scaffold trait. Signatures match RFC §6.1 in spirit; concrete param
/// shapes for `dispatch_*` are finalised in Sub-stage C once the Metal
/// code path exists — until then we only expose `begin_frame` and
/// `finish_frame` so the module tree and fallback/detection glue compile.
pub trait GpuBackend: Send + Sync {
    /// Produce a fresh per-call context. Implementations must NOT cache or
    /// share the returned context across calls (§4.1 (B) invariant).
    fn begin_frame(&self) -> Result<FrameContext, GpuError>;

    /// Consume the per-call context, submitting any pending work. Metal
    /// commits the command buffer here (no `waitUntilCompleted`, §3.3.6).
    /// CUDA performs stream sync here. CPU backend is a no-op.
    fn finish_frame(&self, ctx: FrameContext) -> Result<(), GpuError>;

    /// Human-readable backend tag for logs / About dialog.
    fn name(&self) -> &'static str;
}

/// Runtime selection of the available backend. Sub-stage D will wire up
/// the real detection logic (§4.3 spike); for Sub-stage B we just expose
/// the CPU backend unconditionally.
pub fn default_backend() -> cpu::CpuBackend {
    cpu::CpuBackend::new()
}
