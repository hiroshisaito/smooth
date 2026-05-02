#!/usr/bin/env bash
# Phase 2-A.2 Step 3: manifest-driven goldens fetch + integrity verification.
#
# For each suite under tests/goldens/<suite>/manifest.toml:
#   1. If every .raw file referenced by the manifest already exists locally
#      with the expected SHA256, the suite is considered satisfied.
#   2. Otherwise, if `artifact_url` is set in the manifest, download the
#      tar.zst, verify its SHA256 (`artifact_sha256`), unpack into the suite
#      directory, then re-verify per-file SHA256.
#   3. If files are still missing/mismatched and `artifact_url` is empty
#      (Step 4 hasn't uploaded the artifact yet), exit non-zero with a
#      message pointing at the manual capture path.
#
# Usage:
#   tests/fetch_goldens.sh                # check all suites
#   tests/fetch_goldens.sh v1.4.0-ae2025  # check one suite
#
# Exit codes:
#   0  every requested suite is fully present and SHA256-verified
#   1  a suite is missing files and no artifact_url is configured yet
#   2  download / unpack / SHA256 verification failed
#   3  manifest.toml malformed or unreadable

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GOLDENS_ROOT="$ROOT/tests/goldens"

if ! command -v python3 >/dev/null 2>&1; then
    echo "fetch_goldens.sh: python3 is required (uses stdlib tomllib + hashlib)" >&2
    exit 3
fi

# Pick which suites to process: explicit args override auto-discovery.
if [ "$#" -gt 0 ]; then
    SUITES=("$@")
else
    SUITES=()
    while IFS= read -r d; do
        SUITES+=("$(basename "$d")")
    done < <(find "$GOLDENS_ROOT" -mindepth 1 -maxdepth 1 -type d | sort)
fi

if [ "${#SUITES[@]}" -eq 0 ]; then
    echo "fetch_goldens.sh: no suites found under $GOLDENS_ROOT" >&2
    exit 0
fi

overall_rc=0

for suite in "${SUITES[@]}"; do
    suite_dir="$GOLDENS_ROOT/$suite"
    manifest="$suite_dir/manifest.toml"

    if [ ! -f "$manifest" ]; then
        echo "[$suite] manifest.toml missing at $manifest" >&2
        overall_rc=3
        continue
    fi

    echo "==> [$suite] verifying manifest"

    # Run a Python helper that walks the manifest, checks presence + SHA256,
    # and prints either "OK <suite>" or a list of missing/mismatched files.
    status_output="$(python3 - "$manifest" "$suite_dir" <<'PY'
import hashlib, os, sys, tomllib

manifest_path, suite_dir = sys.argv[1], sys.argv[2]
with open(manifest_path, "rb") as f:
    m = tomllib.load(f)

def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()

missing = []
mismatched = []
verified = 0
for entry in m.get("frames", []):
    for role in ("in", "out"):
        fname = entry[f"{role}_file"]
        sha   = entry[f"{role}_sha256"]
        path  = os.path.join(suite_dir, fname)
        if not os.path.isfile(path):
            missing.append(fname)
            continue
        actual = sha256_of(path)
        if actual != sha:
            mismatched.append((fname, sha, actual))
        else:
            verified += 1

artifact_url = (m.get("suite", {}) or {}).get("artifact_url", "") or ""

print(f"VERIFIED={verified}")
print(f"MISSING={len(missing)}")
print(f"MISMATCHED={len(mismatched)}")
print(f"ARTIFACT_URL={artifact_url}")
for fname in missing:
    print(f"MISSING_FILE\t{fname}")
for fname, want, got in mismatched:
    print(f"MISMATCH_FILE\t{fname}\twant={want}\tgot={got}")
PY
)" || { echo "[$suite] manifest parse failed" >&2; overall_rc=3; continue; }

    verified=$(echo "$status_output" | awk -F= '/^VERIFIED=/{print $2}')
    missing=$(echo "$status_output"  | awk -F= '/^MISSING=/{print $2}')
    mismatched=$(echo "$status_output" | awk -F= '/^MISMATCHED=/{print $2}')
    artifact_url=$(echo "$status_output" | awk -F= '/^ARTIFACT_URL=/{print $2}')

    if [ "$missing" = "0" ] && [ "$mismatched" = "0" ]; then
        echo "[$suite] OK ($verified files SHA256-verified)"
        continue
    fi

    echo "[$suite] verified=$verified missing=$missing mismatched=$mismatched" >&2
    echo "$status_output" | grep -E '^(MISSING_FILE|MISMATCH_FILE)' >&2 | head -20

    if [ -z "$artifact_url" ]; then
        cat >&2 <<EOF
[$suite] artifact_url is empty in manifest.toml.
This means the tar.zst has not yet been uploaded to a GitHub Release
(see Phase 2-A.2 Step 4 in docs/PHASE_2A_STATUS.md). For now, populate
$suite_dir manually from the capture rig used in workbench_history.md
(Phase 1 / Step 3 entries) and re-run this script to verify SHA256.
EOF
        overall_rc=1
        continue
    fi

    # Download path is wired in but currently dormant (artifact_url is empty
    # in v1.4.0-ae2025 until Step 4). Implementation is intentionally simple
    # — curl + tar + zstd — so the only thing Step 4 needs to add is the URL
    # and SHA256 fields in the manifest.
    tarball="$suite_dir/.fetched.tar.zst"
    echo "[$suite] downloading $artifact_url"
    if ! curl -fL --retry 3 -o "$tarball" "$artifact_url"; then
        echo "[$suite] download failed" >&2
        overall_rc=2
        continue
    fi
    artifact_sha256=$(echo "$status_output" | awk -F= '/^ARTIFACT_SHA256=/{print $2}')
    if [ -n "$artifact_sha256" ]; then
        actual_tar_sha=$(shasum -a 256 "$tarball" | awk '{print $1}')
        if [ "$actual_tar_sha" != "$artifact_sha256" ]; then
            echo "[$suite] tarball SHA256 mismatch: want=$artifact_sha256 got=$actual_tar_sha" >&2
            rm -f "$tarball"
            overall_rc=2
            continue
        fi
    fi
    if ! tar --use-compress-program=unzstd -xf "$tarball" -C "$suite_dir" --strip-components=1; then
        echo "[$suite] tar extract failed" >&2
        overall_rc=2
        rm -f "$tarball"
        continue
    fi
    rm -f "$tarball"
    echo "[$suite] re-running verification after extract"
    if ! "$0" "$suite"; then
        overall_rc=2
    fi
done

exit $overall_rc
