#!/usr/bin/env python3
"""Compare two SMDP raw dumps byte-by-byte (with channel-level diff summary).

Usage: python3 compare_raw.py <golden.raw> <new.raw>
Exits 0 when identical, 1 when any pixel differs.
"""
from __future__ import annotations

import struct
import sys
from pathlib import Path

HEADER_FMT = "<4sIIIIIII I f I 5I"  # 64 bytes
HEADER_SIZE = struct.calcsize(HEADER_FMT)
assert HEADER_SIZE == 64, f"expected 64, got {HEADER_SIZE}"


def parse_header(buf: bytes) -> dict:
    (magic, version, width, height, bpc, rowbytes, channels, frame_n,
     params_range, params_line_weight, params_white,
     r0, r1, r2, r3, r4) = struct.unpack(HEADER_FMT, buf[:HEADER_SIZE])
    return {
        "magic": magic,
        "version": version,
        "width": width,
        "height": height,
        "bpc": bpc,
        "rowbytes": rowbytes,
        "channels": channels,
        "frame_n": frame_n,
        "params_range": params_range,
        "params_line_weight": params_line_weight,
        "params_white": params_white,
    }


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    a = Path(argv[1]).read_bytes()
    b = Path(argv[2]).read_bytes()

    ha = parse_header(a)
    hb = parse_header(b)
    if ha["magic"] != b"SMDP" or hb["magic"] != b"SMDP":
        print("ERROR: not SMDP files", file=sys.stderr)
        return 2
    for k in ("width", "height", "bpc", "rowbytes", "channels"):
        if ha[k] != hb[k]:
            print(f"HEADER MISMATCH {k}: {ha[k]} vs {hb[k]}")
            return 1

    pa = a[HEADER_SIZE:]
    pb = b[HEADER_SIZE:]
    if pa == pb:
        print(f"IDENTICAL {len(pa)} bytes  w={ha['width']} h={ha['height']} bpc={ha['bpc']}")
        return 0

    diffs = 0
    max_abs = 0
    for i, (ba, bb) in enumerate(zip(pa, pb)):
        if ba != bb:
            diffs += 1
            max_abs = max(max_abs, abs(ba - bb))
    print(f"DIFFER bytes={diffs}/{len(pa)} ({100 * diffs / len(pa):.3f}%) max_abs_byte_delta={max_abs}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
