// Sub-stage B go-no-go: trait + CPU backend compile and behave sanely.
// Real correctness testing via the trait (CPU regression through trait)
// lands in Sub-stage C once dispatch_* is defined.

use super::*;

#[test]
fn cpu_backend_roundtrip() {
    let b = cpu::CpuBackend::new();
    assert_eq!(b.name(), "cpu");
    let ctx = b.begin_frame().expect("begin_frame");
    b.finish_frame(ctx).expect("finish_frame");
}

#[test]
fn default_backend_is_cpu() {
    let b = default_backend();
    assert_eq!(b.name(), "cpu");
}

#[cfg(target_os = "macos")]
#[test]
fn metal_stub_reports_unavailable() {
    let r = metal::MetalBackend::from_ae_device(std::ptr::null_mut(), std::ptr::null_mut());
    assert!(r.is_err());
}

#[cfg(target_os = "windows")]
#[test]
fn cuda_stub_reports_unavailable() {
    let r = cuda::CudaBackend::from_ae_device(std::ptr::null_mut(), std::ptr::null_mut());
    assert!(r.is_err());
}
