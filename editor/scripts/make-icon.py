#!/usr/bin/env python3
"""Regenerate every Layerfold icon file in assets/app-icon/ (needs Pillow).

The icon is plain polygons, so the SVG and the bitmaps come from one list of shapes. File names stay
the upstream ones: build scripts, tests and packaging read them, and keeping them avoids merge
conflicts with EffectCraft updates.

    python editor/scripts/make-icon.py
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
DIR = ROOT / "assets" / "app-icon"
ID = "ai.storyteller.effectcraft"
FAVICON = ROOT / "apps" / "effectcraft-web" / "web" / "favicon.svg"

TILE = "#171a2b"
RADIUS = 112
# One sheet folded into three layers, back to front, on a 512 x 512 canvas. Each fold is a darker
# strip that joins a layer to the next.
SHAPES = [
    ("#f2b544", [(120, 132), (392, 132), (424, 196), (152, 196)]),
    ("#b97f1f", [(424, 196), (392, 228), (120, 228), (152, 196)]),
    ("#ef6f53", [(120, 228), (392, 228), (424, 292), (152, 292)]),
    ("#a9412c", [(152, 292), (120, 324), (392, 324), (424, 292)]),
    ("#f4efe3", [(120, 324), (392, 324), (424, 388), (152, 388)]),
]


def svg() -> str:
    polys = "".join(f'<polygon fill="{c}" points="{" ".join(f"{x},{y}" for x, y in pts)}"/>' for c, pts in SHAPES)
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">'
        f'<rect width="512" height="512" rx="{RADIUS}" fill="{TILE}"/>{polys}</svg>\n'
    )


def render(size: int, margin: float = 0.0) -> Image.Image:
    """The icon at `size` px, drawn 8x and reduced; `margin` is the transparent share of each edge."""
    big = size * 8
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    scale = big * (1 - 2 * margin) / 512
    off = big * margin
    draw.rounded_rectangle((off, off, big - off - 1, big - off - 1), RADIUS * scale, fill=TILE)
    for colour, pts in SHAPES:
        draw.polygon([(off + x * scale, off + y * scale) for x, y in pts], fill=colour)
    return img.resize((size, size), Image.Resampling.LANCZOS)


def main() -> None:
    text = svg()
    for path in (
        DIR / "effectcraft.svg",
        DIR / "effectcraft-small.svg",
        DIR / "hicolor" / "scalable" / "apps" / f"{ID}.svg",
        FAVICON,
    ):
        path.write_text(text, encoding="utf-8", newline="\n")
    render(1024).save(DIR / "effectcraft-1024.png")
    # macOS keeps Apple's grid: an 824 px body centred on a 1024 canvas.
    apple_margin = (1024 - 824) / 2 / 1024
    render(512, apple_margin).save(DIR / "effectcraft-macos-512.png")
    for size in (16, 24, 32, 48, 64, 128, 256, 512):
        render(size).save(DIR / "hicolor" / f"{size}x{size}" / "apps" / f"{ID}.png")
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    render(256).save(DIR / "effectcraft.ico", sizes=[(s, s) for s in sizes])
    render(1024, apple_margin).save(DIR / "effectcraft.icns")


if __name__ == "__main__":
    main()
