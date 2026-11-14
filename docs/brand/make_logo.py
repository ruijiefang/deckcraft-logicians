#!/usr/bin/env python3
"""Writes the DeckCraft Logicians Edition logo: the mark and the logo with its wordmark, for light
and dark backgrounds.

The mark is DeckCraft's own app icon (the presentation board on its stand, on the orange tile;
`assets/app-icon/hicolor/scalable/apps/ai.storyteller.deckcraft.svg`) with a turnstile (⊨, LaTeX's
`\\vDash`) on the board and a hooded, cloaked logician in front of it pointing a wand at the
turnstile. Everything is drawn here as plain shapes; the wordmark is Inter (OFL-1.1, from
craft-fonts) turned into outlines, so the SVGs need no font.

    python3 docs/brand/make_logo.py [CRAFT_FONTS_DIR]

needs fontTools; CRAFT_FONTS_DIR defaults to ../craft-fonts next to this repository.
"""

import os
import sys

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.ttLib import TTFont

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
FONTS = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(ROOT), "craft-fonts")

ORANGE = "#f26b1d"
INK = "#0b0b0c"
BOARD = "#efe9dc"
CLOAK = "#3a2d6e"
CLOAK_DARK = "#251c4c"
CLOAK_LIGHT = "#5b4aa6"
HOOD = "#120d24"
HOOD_OUTER = "#46388a"
GLOW = "#ffcc4d"


def mark(dark_outline=False):
    """The mark's shapes on a 512 × 512 grid."""
    edge = ' stroke="#f5f2ea" stroke-width="10"' if dark_outline else ""
    return f"""<rect x="16" y="16" width="480" height="480" rx="111.84" fill="{ORANGE}"{edge}/>
<rect x="166" y="328" width="20" height="106" fill="{INK}"/>
<rect x="96" y="426" width="160" height="18" fill="{INK}"/>
<rect x="52" y="96" width="288" height="234" rx="14.64" fill="{BOARD}" stroke="{INK}" stroke-width="12"/>
<rect x="86" y="132" width="140" height="18" fill="{INK}"/>
<rect x="84" y="246" width="34" height="56" fill="{ORANGE}"/>
<rect x="128" y="210" width="34" height="92" fill="{INK}"/>
<rect x="172" y="180" width="34" height="122" fill="{ORANGE}"/>
<rect x="232" y="168" width="19" height="134" fill="{CLOAK}"/>
<rect x="251" y="206" width="68" height="19" fill="{CLOAK}"/>
<rect x="251" y="249" width="68" height="19" fill="{CLOAK}"/>
<path d="M 420 198 C 450 214 462 250 460 292 C 472 338 470 400 466 452 C 448 446 432 458 414 450 C 396 458 378 448 360 456 C 348 460 338 454 330 456 C 334 398 344 340 362 304 C 360 260 384 210 420 198 Z" fill="{CLOAK}"/>
<path d="M 412 326 C 420 376 416 414 406 448" fill="none" stroke="{CLOAK_DARK}" stroke-width="7" stroke-linecap="round"/>
<path d="M 384 336 C 380 376 382 414 388 446" fill="none" stroke="{CLOAK_LIGHT}" stroke-width="5" stroke-linecap="round"/>
<path d="M 420 198 C 448 210 464 244 460 286 C 452 308 432 318 406 318 C 384 318 368 308 364 292 C 362 248 386 210 420 198 Z" fill="{HOOD_OUTER}"/>
<ellipse cx="400" cy="264" rx="21" ry="28" transform="rotate(-8 400 264)" fill="{HOOD}"/>
<path d="M 381 238 Q 369 268 386 298" fill="none" stroke="{CLOAK_LIGHT}" stroke-width="5" stroke-linecap="round"/>
<circle cx="392" cy="262" r="3.8" fill="{GLOW}"/>
<circle cx="408" cy="259" r="3.8" fill="{GLOW}"/>
<circle cx="408" cy="322" r="6.5" fill="{GLOW}" stroke="{INK}" stroke-width="2"/>
<path d="M 380 330 Q 354 332 338 352" fill="none" stroke="{CLOAK}" stroke-width="22" stroke-linecap="round"/>
<line x1="336" y1="354" x2="300" y2="292" stroke="{INK}" stroke-width="7" stroke-linecap="round"/>
<path d="M 298 268 L 302 283 L 317 288 L 302 293 L 298 308 L 294 293 L 279 288 L 294 283 Z" fill="{GLOW}" stroke="{INK}" stroke-width="2" stroke-linejoin="round"/>"""


class Font:
    def __init__(self, path):
        self.font = TTFont(path)
        self.glyphs = self.font.getGlyphSet()
        self.cmap = self.font.getBestCmap()
        self.upem = self.font["head"].unitsPerEm
        self.hmtx = self.font["hmtx"]

    def text(self, s, x, y, size, fill, tracking=0.0):
        """`s` as outlined glyphs from (x, y) on the baseline; returns the SVG and the end x."""
        k = size / self.upem
        out = []
        for ch in s:
            name = self.cmap.get(ord(ch))
            if name is None:
                continue
            pen = SVGPathPen(self.glyphs)
            self.glyphs[name].draw(pen)
            d = pen.getCommands()
            if d:
                out.append(f'<path transform="translate({x:.2f} {y:.2f}) scale({k:.5f} {-k:.5f})" d="{d}" fill="{fill}"/>')
            x += self.hmtx[name][0] * k + tracking
        return "\n".join(out), x - tracking


def write(name, body, w, h):
    svg = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:.0f} {h:.0f}" width="{w:.0f}" height="{h:.0f}">\n{body}\n</svg>\n'
    with open(os.path.join(HERE, name), "w", encoding="utf-8") as f:
        f.write(svg)


def main():
    bold = Font(os.path.join(FONTS, "fonts", "inter", "Inter-Bold.ttf"))
    semi = Font(os.path.join(FONTS, "fonts", "inter", "Inter-SemiBold.ttf"))
    write("logicians-mark.svg", mark(), 512, 512)
    for variant, word, edition in [("logicians-logo.svg", "#1b1b1f", CLOAK), ("logicians-logo-white.svg", "#f5f2ea", "#c9bcff")]:
        size = 280
        scale = size / 512
        emblem = f'<g transform="translate(10 10) scale({scale:.5f})">{mark()}</g>'
        x0 = size + 46
        title, end1 = bold.text("DeckCraft", x0, 172, 140, word)
        sub, end2 = semi.text("LOGICIANS EDITION", x0 + 4, 248, 50, edition, tracking=7.5)
        w = max(end1, end2) + 24
        write(variant, f"{emblem}\n{title}\n{sub}", w, size + 20)


if __name__ == "__main__":
    main()
