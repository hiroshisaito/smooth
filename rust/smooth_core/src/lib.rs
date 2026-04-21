// smooth_core Rust crate (Phase 2-C scaffold)
//
// Step 1 exposes only a linkage probe. Real processing will migrate
// step-by-step in Phase 2-C Step 2 onwards.

#[no_mangle]
pub extern "C" fn smooth_core_version() -> u32 {
    0x0002_0000
}
