#!/usr/bin/env python3
"""Compose the STELLAR COMPOSED figure: a 'REBALANCING VAULT' ring
wrapping the Aquarius / Reflector / DeFindex marks.

Sources (fetched once, kept in ./logos/):
  aquarius.png  — aqua.network favicon (purple wave mark)
  reflector.png — reflector-network GitHub org avatar (S-orbit on dark)
  defindex.png  — defindex.io favicon (teal leaf mark)

Run: ../.venv/bin/python assets/make_composed_figure.py
Out: assets/composed_figure.png
"""
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
OUT = HERE / "composed_figure.png"
LOGOS = {
    "AQUARIUS": HERE / "logos/aquarius.png",
    "DEFINDEX": HERE / "logos/defindex.png",
    "REFLECTOR": HERE / "logos/reflector.png",
}

W, H = 1700, 1100
INK = (26, 26, 30, 255)          # near-black, matches slide ink
PAPER = (255, 255, 255, 0)       # transparent
YELLOW = (255, 210, 63, 255)     # hackmeridian pill yellow
GRAY = (200, 200, 205, 255)      # connector lines
SUB = (90, 90, 95, 255)

FONT_B = "/usr/share/fonts/liberation/LiberationSans-Bold.ttf"
FONT_R = "/usr/share/fonts/liberation/LiberationSans-Regular.ttf"


def rounded_chip(img, size, radius_ratio=0.22):
    """Square logo -> rounded-corner chip at `size` px."""
    im = img.convert("RGBA").resize((size, size), Image.LANCZOS)
    mask = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(mask)
    d.rounded_rectangle([0, 0, size - 1, size - 1],
                        radius=int(size * radius_ratio), fill=255)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(im, (0, 0), mask)
    return out


def main():
    cv = Image.new("RGBA", (W, H), PAPER)
    dr = ImageDraw.Draw(cv)

    cx, cy = W // 2, 610
    RX, RY = 735, 400             # ring: horizontal ellipse
    dr.ellipse([cx - RX, cy - RY, cx + RX, cy + RY],
               outline=INK, width=15)

    # clockwise arrowheads on the ring at left and right midpoints —
    # the loop that keeps the position in range
    for sign in (-1, 1):
        x = cx + sign * RX
        y = cy
        # tangent at side points is vertical; clockwise => down at left,
        # up at right
        dy = 1 if sign < 0 else -1
        dr.polygon([(x, y + dy * 55), (x - 26, y - dy * 18),
                    (x + 26, y - dy * 18)], fill=INK)

    # connector bar behind the chips
    chip = 250
    xs = [cx - 390, cx, cx + 390]
    names = list(LOGOS)
    dr.line([xs[0], cy, xs[-1], cy], fill=GRAY, width=6)

    f_name = ImageFont.truetype(FONT_B, 46)
    f_pill = ImageFont.truetype(FONT_B, 58)
    f_sub = ImageFont.truetype(FONT_R, 34)

    subs = {"AQUARIUS": "CL liquidity",
            "DEFINDEX": "vault custody",
            "REFLECTOR": "oracle referee"}

    for name, px in zip(names, xs):
        logo = Image.open(LOGOS[name])
        ch = rounded_chip(logo, chip)
        cv.paste(ch, (int(px - chip / 2), int(cy - chip / 2)), ch)
        w = dr.textlength(name, font=f_name)
        dr.text((px - w / 2, cy + chip / 2 + 22), name,
                font=f_name, fill=INK)
        w = dr.textlength(subs[name], font=f_sub)
        dr.text((px - w / 2, cy + chip / 2 + 78), subs[name],
                font=f_sub, fill=SUB)

    # "REBALANCING VAULT" pill straddling the ring's top edge
    label = "REBALANCING VAULT"
    tw = dr.textlength(label, font=f_pill)
    pw, ph = tw + 110, 112
    px0, py0 = cx - pw / 2, cy - RY - ph / 2
    dr.rounded_rectangle([px0, py0, px0 + pw, py0 + ph],
                         radius=ph // 2, fill=YELLOW)
    asc, desc = f_pill.getmetrics()
    dr.text((cx - tw / 2, cy - RY - (asc + desc) / 2 - 6), label,
            font=f_pill, fill=INK)

    cv.save(OUT)
    print(OUT, cv.size)


if __name__ == "__main__":
    main()
