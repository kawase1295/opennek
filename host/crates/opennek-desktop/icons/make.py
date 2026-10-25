"""Draws the app icons from the shape of the SVG `opennek install` writes (host/crates/opennek-cli/src/install.rs).

Run with Pillow from this directory: python3 make.py
"""

import math

from PIL import Image, ImageDraw

BLUE = (0x22, 0x88, 0xAA, 255)
S = 1024  # drawn large, then scaled down
K = S / 32  # the SVG's viewBox is 32 x 32


def arc(draw, cx, start, end):
    """One of the two 'waves': an 11-unit arc through (8, 8) and (8, 24) mirrored, 3 units wide, round caps"""
    r, w = 11 * K, 3 * K
    # Pillow draws the width inwards from the box, an SVG stroke is centred on the path
    o = r + w / 2
    draw.arc([cx * K - o, 16 * K - o, cx * K + o, 16 * K + o], start, end, fill=BLUE, width=round(w))
    for a in (start, end):
        x = cx * K + r * math.cos(math.radians(a))
        y = 16 * K + r * math.sin(math.radians(a))
        draw.ellipse([x - w / 2, y - w / 2, x + w / 2, y + w / 2], fill=BLUE)


def icon():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.ellipse([11 * K, 11 * K, 21 * K, 21 * K], fill=BLUE)
    half = math.degrees(math.atan2(8, math.sqrt(11**2 - 8**2)))  # where the arc meets y = 8 and y = 24
    arc(d, 8 + math.sqrt(11**2 - 8**2), 180 - half, 180 + half)
    arc(d, 24 - math.sqrt(11**2 - 8**2), -half, half)
    return img


big = icon()
for name, size in [("32x32.png", 32), ("128x128.png", 128), ("256x256.png", 256), ("icon.png", 512)]:
    big.resize((size, size), Image.LANCZOS).save(name)
big.resize((256, 256), Image.LANCZOS).save("icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
