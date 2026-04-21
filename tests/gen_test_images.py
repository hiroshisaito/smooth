#!/usr/bin/env python3
"""Generate synthetic test fixtures for smooth-mod-v1.5.0 regression testing.

Outputs are pixel-art style PNGs with hard-edged shapes where smooth's
corner-detection + anti-aliasing should produce deterministic non-trivial output.

Run: python3 tests/gen_test_images.py
Outputs: tests/fixtures/*.png
Requires: pillow (pip install pillow)
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

try:
    from PIL import Image, ImageDraw
except ImportError:
    sys.stderr.write("pillow not installed. Run: pip install pillow\n")
    sys.exit(1)

FIXTURES = Path(__file__).resolve().parent / "fixtures"
FIXTURES.mkdir(parents=True, exist_ok=True)

# (name, size, generator)
# Shapes are chosen to exercise all 4 corner orientations (up/down × h/v)
# and irregular "lack" cases where smooth's adjacent-pixel logic kicks in.


def pixel_triangle(size: int) -> Image.Image:
    """Right triangle made of hard pixels — stepped hypotenuse exercises all corner types."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    px = img.load()
    for y in range(size):
        for x in range(size):
            if x + y < size:
                px[x, y] = (50, 200, 80, 255)
    return img


def pixel_diamond(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    px = img.load()
    cx = cy = size // 2
    r = size // 2 - 1
    for y in range(size):
        for x in range(size):
            if abs(x - cx) + abs(y - cy) <= r:
                px[x, y] = (210, 90, 140, 255)
    return img


def pixel_checker_runs(size: int) -> Image.Image:
    """Staircase of varying run-lengths — stresses the count/lack correction path."""
    img = Image.new("RGBA", (size, size), (255, 255, 255, 255))
    d = ImageDraw.Draw(img)
    y = 0
    run = 1
    toggle = True
    while y < size:
        color = (20, 40, 80, 255) if toggle else (240, 220, 60, 255)
        d.rectangle([0, y, run * 3, y + run], fill=color)
        y += run
        run = 1 + (run % 7)
        toggle = not toggle
    return img


def pixel_text_like(size: int) -> Image.Image:
    """Blocky 'E' — many 90-degree corners (up/down + lack) in a small area."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    m = size // 8
    col = (30, 30, 30, 255)
    d.rectangle([m, m, 2 * m, 7 * m], fill=col)              # spine
    d.rectangle([m, m, 6 * m, 2 * m], fill=col)              # top arm
    d.rectangle([m, 4 * m, 5 * m, 5 * m], fill=col)          # mid arm
    d.rectangle([m, 6 * m, 6 * m, 7 * m], fill=col)          # bottom arm
    return img


def gradient_noop(size: int) -> Image.Image:
    """Smooth gradient — no hard edges. Expect near-identity output (sanity)."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    px = img.load()
    for y in range(size):
        for x in range(size):
            r = int(255 * x / max(size - 1, 1))
            b = int(255 * y / max(size - 1, 1))
            px[x, y] = (r, 128, b, 255)
    return img


def tile(base: Image.Image, target: tuple[int, int]) -> Image.Image:
    tw, th = target
    out = Image.new("RGBA", (tw, th), (0, 0, 0, 0))
    for y in range(0, th, base.height):
        for x in range(0, tw, base.width):
            out.paste(base, (x, y))
    return out


def main() -> int:
    small = [
        ("triangle_64",  pixel_triangle(64)),
        ("diamond_64",   pixel_diamond(64)),
        ("staircase_64", pixel_checker_runs(64)),
        ("letterE_64",   pixel_text_like(64)),
        ("gradient_64",  gradient_noop(64)),
    ]

    for name, img in small:
        path = FIXTURES / f"{name}.png"
        img.save(path)
        print(f"wrote {path}")

    # Tiled large versions for speed benchmarking (HD / 4K)
    tri_tile = pixel_triangle(32)
    tri_hd = tile(tri_tile, (1920, 1080))
    tri_hd.save(FIXTURES / "triangle_tiled_hd.png")
    print(f"wrote {FIXTURES / 'triangle_tiled_hd.png'}")

    tri_4k = tile(tri_tile, (3840, 2160))
    tri_4k.save(FIXTURES / "triangle_tiled_4k.png")
    print(f"wrote {FIXTURES / 'triangle_tiled_4k.png'}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
