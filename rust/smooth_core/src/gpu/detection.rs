// GPU backend detection (RFC §4.3 / §5.3.1 / §3.3.1 (e)).
//
// `GPU_BACKEND_USABLE` is the single backend-level "is there a usable GPU
// path in this process?" state. It is set at `PF_Cmd_GLOBAL_SETUP` (once
// per process) and again at `PF_Cmd_GPU_DEVICE_SETUP` (per device the host
// offers to us). PreRender's 5-condition AND (§3.3.1) reads this to decide
// whether to raise `PF_RenderOutputFlag_GPU_RENDER_POSSIBLE`.
//
// Kept separate from `fallback::GPU_FALLEN`: this is process-wide backend
// health (e.g., "no NVIDIA driver"); fallback is per-instance (e.g., "this
// particular layer had an OOM once this span").

use std::sync::atomic::{AtomicBool, Ordering};

static GPU_BACKEND_USABLE: AtomicBool = AtomicBool::new(false);

/// Sub-stage D wires this to the real detection logic (§4.3 spike result).
/// For Sub-stage B the default is `false` — production enables it from the
/// Effect.cpp `PF_Cmd_GLOBAL_SETUP` handler.
pub fn set_backend_usable(ok: bool) {
    GPU_BACKEND_USABLE.store(ok, Ordering::Release);
}

pub fn is_backend_usable() -> bool {
    GPU_BACKEND_USABLE.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_backend_usable() {
        let prev = is_backend_usable();
        set_backend_usable(true);
        assert!(is_backend_usable());
        set_backend_usable(false);
        assert!(!is_backend_usable());
        set_backend_usable(prev);
    }
}
