#!/usr/bin/env python3
"""Draws the README wordmark: "spool" in Geist Bold, as outlined SVG paths
(GitHub shows SVGs as images, which can't load web fonts).
Needs fontTools and brotli (pip install fonttools brotli)."""
from pathlib import Path

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

ROOT = Path(__file__).resolve().parent.parent
font = instantiateVariableFont(TTFont(ROOT / "apps/web/public/fonts/Geist-Variable.woff2"), {"wght": 700})
glyphs = font.getGlyphSet()
cmap = font.getBestCmap()
upm = font["head"].unitsPerEm
size = 96.0
scale = size / upm
tracking = -0.05 * upm  # the UI's letter-spacing: -0.05em

pen = SVGPathPen(glyphs)
x = 0.0
for ch in "spool":
    g = cmap[ord(ch)]
    glyphs[g].draw(TransformPen(pen, (scale, 0, 0, -scale, x * scale, 0)))
    x += glyphs[g].width + tracking
width = (x - tracking) * scale
top, bottom, pad = 0.80 * size, 0.30 * size, 8
w, h = width + 2 * pad, top + bottom + 2 * pad
for name, color in [("wordmark-dark", "#ffffff"), ("wordmark-light", "#000000")]:
    (ROOT / f"docs/assets/{name}.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{-pad} {-top - pad:.1f} {w:.1f} {h:.1f}" '
        f'width="{w * 0.75:.0f}" height="{h * 0.75:.0f}" role="img" aria-label="spool">'
        f'<path fill="{color}" d="{pen.getCommands()}"/></svg>\n'
    )
print("wrote docs/assets/wordmark-{dark,light}.svg")
