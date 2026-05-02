#!/usr/bin/env python3
# Phase 2-A.2 Step 4 capture utility: AE-exported EXR -> SMDP v2 .raw
#
# WHAT THIS DOES
# --------------
# Given two OpenEXR files captured from the same Mac AE 2025 32bpc
# composition --- one of the input layer pre-effect, one of the output
# layer post-smooth --- emit a pair of SMDP v2 .raw fixtures that the
# regression harness can replay through smooth_core::process<PF_PixelFloat>.
#
# Why EXR-pair instead of bench dump?
# -----------------------------------
# 8/16bpc goldens are captured by the bench-instrumented plugin
# (SMOOTH_BENCH=1 build), which writes SMDP straight from the render
# call. For 32bpc we instead require AE Render Queue --> EXR export so
# that re-capture does not need a custom build of the plugin: anyone
# with AE 2025 + Python + OpenEXR can rebuild the goldens. See
# docs/PHASE_2A_GPU_RFC.md S3.2.5 step 2 for the rationale and S3.2.6
# for the manifest schema this script produces fixtures for.
#
# PIPELINE
# --------
#   AE Render Queue (32bpc project)
#     -> input_NNNN.exr   (layer 0 only, no smooth)
#     -> output_NNNN.exr  (final comp output, smooth applied)
#   tests/capture_32bpc.py --in-exr ... --out-exr ... --frame-n N --range R ...
#     -> tests/goldens/v1.6.0-32bpc/frame_NNNN_in.raw   (SMDP v2, bpc=32)
#     -> tests/goldens/v1.6.0-32bpc/frame_NNNN_out.raw  (SMDP v2, bpc=32)
#
# The slider param values (`--range`, `--line-weight`, `--white`) must
# match what was set in the AE comp -- they are recorded in the SMDP
# header so the regression harness can reconstruct the same Params and
# replay the smooth on the input to check it matches the captured
# output.
#
# CHANNEL ORDER
# -------------
# OpenEXR: separate R, G, B, A channels (potentially in any order in
# the file; we pick by name).
# AE PF_PixelFloat: contiguous { alpha, red, green, blue } per pixel.
# This script always writes ARGB regardless of EXR channel order.
#
# OVERBRIGHT / NaN / Inf
# ----------------------
# 32bpc f32 may carry values outside [0, 1] (overbright HDR) or
# non-finite values (NaN/Inf). The script does NOT clip and does NOT
# replace -- whatever AE wrote is preserved verbatim, since the smooth
# core has unit tests confirming its 32bpc path tolerates overbright
# and treats NaN inputs as "leave alone" via PartialEq miss. The script
# DOES count and report these so a sanity glance at the output catches
# bad capture rigs (see --verbose).
#
# PREMULTIPLICATION
# -----------------
# AE's 32bpc working space uses straight alpha. EXR can be written
# either way. We do not re-multiply or un-multiply; the bytes go
# through unchanged. If you suspect the EXR was saved premultiplied,
# inspect the layer settings in AE before exporting.
#
# DEPENDENCIES
# ------------
# python3 >= 3.11 (uses tomllib + dataclasses)
# OpenEXR + numpy via tests/requirements-capture.txt:
#     pip install -r tests/requirements-capture.txt
# Run inside tests/.venv to keep the dev image clean.
#
# USAGE
# -----
# Single-frame:
#     ./capture_32bpc.py \
#         --frame-n 200 \
#         --in-exr  /tmp/exr/input_0200.exr \
#         --out-exr /tmp/exr/output_0200.exr \
#         --range 12.0 --line-weight 0.5351 \
#         --output-dir tests/goldens/v1.6.0-32bpc/
#
# Batch via TOML (Step 4 will add a capture_config.toml committed alongside
# the manifest -- this script supports it now so the operator can dry-run):
#     ./capture_32bpc.py --config tests/capture_config_32bpc.toml
#
# Self-test (no EXR, no OpenEXR import; verifies SMDP write/read symmetry
# using a synthetic numpy array):
#     ./capture_32bpc.py --self-test
#
# EXIT CODES
# ----------
#   0  success
#   1  invalid CLI args / config schema
#   2  EXR file unreadable / channel layout unexpected
#   3  pixel buffer dimensions disagreed between input/output
#   4  output write failed
#   5  self-test failure

from __future__ import annotations

import argparse
import os
import struct
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Optional, Tuple


SMDP_MAGIC = b"SMDP"
SMDP_VERSION = 2
SMDP_HEADER_SIZE = 64
PIXEL32_SIZE = 16  # 4 channels * 4 bytes (f32)


@dataclass
class FrameParams:
    n: int
    range_pct: float       # raw slider value 0..100 (matches PARAM_RANGE.fs_d.value)
    line_weight: float     # raw slider value 0..1
    white: bool            # PARAM_WHITE_OPTION
    in_exr: Path
    out_exr: Path
    width_override: Optional[int] = None
    height_override: Optional[int] = None


def smdp_v2_header(width: int, height: int, frame_n: int,
                   range_pct: float, line_weight: float, white: bool) -> bytes:
    # SMDP v2 layout (matches bench.h::DumpHeader):
    #   0  magic[4]               "SMDP"
    #   4  version u32             2
    #   8  width u32
    #  12  height u32
    #  16  bpc u32                 32
    #  20  rowbytes u32            width * 16
    #  24  channels u32            4
    #  28  frame_n u32
    #  32  params_range u32        0 (32bpc path uses params_range_f32 instead)
    #  36  params_line_weight f32
    #  40  params_white u32
    #  44  params_range_f32 f32    raw_slider * 4 channels / 100
    #  48  reserved[4] u32         all zero
    rowbytes = width * PIXEL32_SIZE
    range_f32 = (range_pct * 4.0) / 100.0
    hdr = bytearray(SMDP_HEADER_SIZE)
    hdr[0:4] = SMDP_MAGIC
    struct.pack_into("<I", hdr, 4,  SMDP_VERSION)
    struct.pack_into("<I", hdr, 8,  width)
    struct.pack_into("<I", hdr, 12, height)
    struct.pack_into("<I", hdr, 16, 32)
    struct.pack_into("<I", hdr, 20, rowbytes)
    struct.pack_into("<I", hdr, 24, 4)
    struct.pack_into("<I", hdr, 28, frame_n)
    struct.pack_into("<I", hdr, 32, 0)
    struct.pack_into("<f", hdr, 36, line_weight)
    struct.pack_into("<I", hdr, 40, 1 if white else 0)
    struct.pack_into("<f", hdr, 44, range_f32)
    return bytes(hdr)


def read_exr_rgba(path: Path):
    """Returns (width, height, rgba_array_f32) where rgba_array_f32 has shape
    (height, width, 4) in R,G,B,A order. Imports OpenEXR + numpy lazily so
    --self-test works without them installed."""
    try:
        import OpenEXR
        import Imath
        import numpy as np
    except ImportError as e:
        raise SystemExit(
            f"capture_32bpc.py: missing dependency ({e}). "
            "Install via `pip install -r tests/requirements-capture.txt`."
        )

    if not path.exists():
        raise SystemExit(f"EXR not found: {path}")

    f = OpenEXR.InputFile(str(path))
    header = f.header()
    dw = header["dataWindow"]
    width  = dw.max.x - dw.min.x + 1
    height = dw.max.y - dw.min.y + 1
    channels = header["channels"]
    needed = ("R", "G", "B", "A")
    missing = [c for c in needed if c not in channels]
    if missing:
        raise SystemExit(
            f"{path}: missing channels {missing}; have {sorted(channels.keys())}. "
            "Re-export from AE with RGBA enabled (Output Module > Format Options)."
        )

    pt = Imath.PixelType(Imath.PixelType.FLOAT)
    raw = {c: np.frombuffer(f.channel(c, pt), dtype=np.float32).reshape(height, width)
           for c in needed}
    rgba = np.stack([raw["R"], raw["G"], raw["B"], raw["A"]], axis=2)  # HxWx4
    return width, height, rgba


def rgba_to_argb_pixels(rgba_f32) -> bytes:
    """rgba_f32 shape: (H, W, 4) in R,G,B,A order. Returns flat bytes in
    AE PF_PixelFloat layout (ARGB per pixel, contiguous rows)."""
    import numpy as np
    h, w, c = rgba_f32.shape
    assert c == 4, f"expected RGBA, got {c} channels"
    argb = np.empty_like(rgba_f32)
    argb[..., 0] = rgba_f32[..., 3]  # A
    argb[..., 1] = rgba_f32[..., 0]  # R
    argb[..., 2] = rgba_f32[..., 1]  # G
    argb[..., 3] = rgba_f32[..., 2]  # B
    # f32 little-endian byte order matches PF_PixelFloat on Intel/Apple Silicon.
    # tobytes() preserves C-order so rows are contiguous, matching rowbytes = w*16.
    return argb.astype("<f4").tobytes()


def report_pixel_health(label: str, rgba_f32) -> Tuple[int, int, float, float]:
    import numpy as np
    nan_count = int(np.isnan(rgba_f32).sum())
    inf_count = int(np.isinf(rgba_f32).sum())
    finite = rgba_f32[np.isfinite(rgba_f32)]
    vmin = float(finite.min()) if finite.size else 0.0
    vmax = float(finite.max()) if finite.size else 0.0
    overbright = (rgba_f32[..., :3] > 1.0).sum()
    print(f"  {label}: NaN={nan_count} Inf={inf_count} "
          f"min_finite={vmin:.4g} max_finite={vmax:.4g} "
          f"overbright_rgb={int(overbright)}")
    return nan_count, inf_count, vmin, vmax


def write_smdp_pair(out_dir: Path, params: FrameParams, verbose: bool) -> int:
    in_w,  in_h,  in_rgba  = read_exr_rgba(params.in_exr)
    out_w, out_h, out_rgba = read_exr_rgba(params.out_exr)
    if (in_w, in_h) != (out_w, out_h):
        print(f"frame {params.n}: dimension mismatch in={in_w}x{in_h} "
              f"out={out_w}x{out_h}", file=sys.stderr)
        return 3
    width  = params.width_override  or in_w
    height = params.height_override or in_h

    if verbose:
        print(f"frame {params.n}: {width}x{height} bpc=32 "
              f"range_pct={params.range_pct} lw={params.line_weight} "
              f"white={params.white}")
        report_pixel_health("input ", in_rgba)
        report_pixel_health("output", out_rgba)

    in_bytes  = rgba_to_argb_pixels(in_rgba)
    out_bytes = rgba_to_argb_pixels(out_rgba)
    expected_size = width * height * PIXEL32_SIZE
    if len(in_bytes) != expected_size or len(out_bytes) != expected_size:
        print(f"frame {params.n}: pixel buffer size mismatch "
              f"in={len(in_bytes)} out={len(out_bytes)} expect={expected_size}",
              file=sys.stderr)
        return 4

    hdr = smdp_v2_header(width, height, params.n,
                         params.range_pct, params.line_weight, params.white)
    out_dir.mkdir(parents=True, exist_ok=True)
    in_path  = out_dir / f"frame_{params.n:04d}_in.raw"
    out_path = out_dir / f"frame_{params.n:04d}_out.raw"
    try:
        with open(in_path, "wb") as f:
            f.write(hdr); f.write(in_bytes)
        with open(out_path, "wb") as f:
            f.write(hdr); f.write(out_bytes)
    except OSError as e:
        print(f"frame {params.n}: write failed: {e}", file=sys.stderr)
        return 4

    print(f"wrote {in_path}")
    print(f"wrote {out_path}")
    return 0


def load_config(path: Path):
    import tomllib
    with open(path, "rb") as f:
        cfg = tomllib.load(f)
    base_dir = Path(cfg.get("exr_base_dir", path.parent)).expanduser()
    out_dir  = Path(cfg["output_dir"]).expanduser()
    frames = []
    for entry in cfg.get("frames", []):
        frames.append(FrameParams(
            n=int(entry["n"]),
            range_pct=float(entry["range"]),
            line_weight=float(entry["line_weight"]),
            white=bool(entry["white"]),
            in_exr=base_dir / entry["in_exr"],
            out_exr=base_dir / entry["out_exr"],
            width_override=entry.get("width_override"),
            height_override=entry.get("height_override"),
        ))
    return out_dir, frames


def self_test() -> int:
    """Verify SMDP v2 header round-trip + RGBA->ARGB reorder using a synthetic
    numpy array. Skips the EXR path so it can run without OpenEXR installed,
    but still imports numpy."""
    try:
        import numpy as np
    except ImportError:
        print("self-test requires numpy", file=sys.stderr)
        return 5

    width, height = 8, 4
    rgba = np.zeros((height, width, 4), dtype=np.float32)
    # Set distinct values so reorder bugs are obvious.
    rgba[..., 0] = 0.10  # R
    rgba[..., 1] = 0.20  # G
    rgba[..., 2] = 0.30  # B
    rgba[..., 3] = 0.40  # A
    # Sprinkle one overbright + one NaN to exercise health reporting.
    rgba[0, 0, 0] = 2.5
    rgba[1, 1, 1] = float("nan")

    pixels = rgba_to_argb_pixels(rgba)
    assert len(pixels) == width * height * PIXEL32_SIZE

    # Decode pixel (0, 0): expect ARGB = (0.40, 2.5, 0.20, 0.30)
    a, r, g, b = struct.unpack_from("<ffff", pixels, 0)
    assert abs(a - 0.40) < 1e-6, a
    assert abs(r - 2.50) < 1e-6, r
    assert abs(g - 0.20) < 1e-6, g
    assert abs(b - 0.30) < 1e-6, b

    hdr = smdp_v2_header(width=width, height=height, frame_n=42,
                         range_pct=12.5, line_weight=0.535, white=True)
    assert len(hdr) == SMDP_HEADER_SIZE
    assert hdr[0:4] == SMDP_MAGIC
    (version,) = struct.unpack_from("<I", hdr, 4)
    assert version == 2, version
    (bpc,) = struct.unpack_from("<I", hdr, 16)
    assert bpc == 32
    (rowbytes,) = struct.unpack_from("<I", hdr, 20)
    assert rowbytes == width * PIXEL32_SIZE
    (range_u32,)  = struct.unpack_from("<I", hdr, 32)
    assert range_u32 == 0, "32bpc path must zero out u32 range"
    (range_f32,) = struct.unpack_from("<f", hdr, 44)
    expected_range_f32 = 12.5 * 4.0 / 100.0
    assert abs(range_f32 - expected_range_f32) < 1e-6, range_f32

    print("self-test OK (SMDP v2 header layout + RGBA->ARGB reorder)")
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description="EXR -> SMDP v2 capture for 32bpc smooth goldens")
    p.add_argument("--frame-n", type=int)
    p.add_argument("--in-exr", type=Path)
    p.add_argument("--out-exr", type=Path)
    p.add_argument("--range", type=float, dest="range_pct",
                   help="PARAM_RANGE raw slider value (0..100)")
    p.add_argument("--line-weight", type=float)
    p.add_argument("--white", action="store_true")
    p.add_argument("--width-override", type=int, default=None)
    p.add_argument("--height-override", type=int, default=None)
    p.add_argument("--output-dir", type=Path,
                   default=Path("tests/goldens/v1.6.0-32bpc"))
    p.add_argument("--config", type=Path,
                   help="Batch mode: read frames from a TOML config file")
    p.add_argument("--verbose", "-v", action="store_true")
    p.add_argument("--self-test", action="store_true",
                   help="Verify SMDP/reorder logic without touching disk or OpenEXR")
    args = p.parse_args()

    if args.self_test:
        return self_test()

    if args.config:
        out_dir, frames = load_config(args.config)
        rc = 0
        for fp in frames:
            rc = max(rc, write_smdp_pair(out_dir, fp, verbose=args.verbose))
        return rc

    if args.frame_n is None or args.in_exr is None or args.out_exr is None \
       or args.range_pct is None or args.line_weight is None:
        p.error("single-frame mode requires --frame-n --in-exr --out-exr --range --line-weight")
    fp = FrameParams(
        n=args.frame_n,
        range_pct=args.range_pct,
        line_weight=args.line_weight,
        white=args.white,
        in_exr=args.in_exr,
        out_exr=args.out_exr,
        width_override=args.width_override,
        height_override=args.height_override,
    )
    return write_smdp_pair(args.output_dir, fp, verbose=args.verbose)


if __name__ == "__main__":
    sys.exit(main())
