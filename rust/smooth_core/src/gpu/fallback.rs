// Plugin-global fallback state (RFC §2.4, §3.3.1).
//
// `GPU_FALLEN` is a `DashMap<u128, AtomicBool>` keyed by the per-instance
// UUID stored in sequence_data. Once any frame of an instance fails GPU
// render within a SETUP/RESETUP span, the entry is set and every subsequent
// PreRender for that instance declines to set `GPU_RENDER_POSSIBLE` (→ AE
// routes to CPU SmartRender). `SEQUENCE_SETDOWN` removes the entry.
//
// This is Sub-stage B scaffold — the map lives here, but the C++ FFI layer
// that queries / toggles it is wired in Sub-stage C.

use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};

pub static GPU_FALLEN: Lazy<DashMap<u128, AtomicBool>> = Lazy::new(DashMap::new);

/// True if GPU has been marked fallen for this instance at any point in the
/// current SETUP/RESETUP span (entry is removed at SETDOWN).
pub fn is_fallen(uuid: u128) -> bool {
    GPU_FALLEN
        .get(&uuid)
        .map(|e| e.value().load(Ordering::Relaxed))
        .unwrap_or(false)
}

/// Mark the instance as fallen. Idempotent across threads (DashMap's shard
/// lock plus the atomic's Relaxed store are sufficient — there is no
/// ordering requirement beyond visibility, §4.1 confirms AE serialises
/// GPU render calls per instance anyway).
pub fn mark_fallen(uuid: u128) {
    GPU_FALLEN
        .entry(uuid)
        .or_insert_with(|| AtomicBool::new(false))
        .store(true, Ordering::Relaxed);
}

/// Called from SEQUENCE_SETDOWN to clean up the entry so a fresh SETUP
/// (e.g., project reopen) does not carry stale fallen state.
pub fn forget(uuid: u128) {
    GPU_FALLEN.remove(&uuid);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallen_lifecycle() {
        let uuid = 0xDEADBEEFu128;
        assert!(!is_fallen(uuid));
        mark_fallen(uuid);
        assert!(is_fallen(uuid));
        forget(uuid);
        assert!(!is_fallen(uuid));
    }

    #[test]
    fn mark_fallen_idempotent() {
        let uuid = 0xFEEDFACEu128;
        mark_fallen(uuid);
        mark_fallen(uuid);
        assert!(is_fallen(uuid));
        forget(uuid);
    }
}
