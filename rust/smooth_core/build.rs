// Build-time capture of the git short SHA + dirty flag for the build-id UI.
// The resulting string is stamped into the crate via `cargo:rustc-env` and
// surfaced over FFI by `smooth_core_build_id()`.

use std::process::Command;

fn run_git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    Some(s.trim().to_string())
}

fn main() {
    let sha = run_git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());

    // "dirty" means the working tree differs from HEAD. `git diff --quiet HEAD`
    // exits 0 when clean and 1 when dirty; it fails (e.g., no git repo) with
    // other codes, which we treat as clean to avoid false positives in zipped
    // source builds.
    let dirty = Command::new("git")
        .args(["diff", "--quiet", "HEAD"])
        .status()
        .ok()
        .map(|s| s.code() == Some(1))
        .unwrap_or(false);

    let combined = if dirty {
        format!("{sha}+dirty")
    } else {
        sha
    };

    println!("cargo:rustc-env=SMOOTH_CORE_GIT_SHA={combined}");

    // Rebuild when HEAD moves (branch switch / new commit / amend).
    // Paths are relative to the crate manifest at rust/smooth_core/.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
}
