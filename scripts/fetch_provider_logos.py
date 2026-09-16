#!/usr/bin/env python3
"""Fetch and standardize provider brand logos into assets/provider-logos/.

Every logo ships as a 64x64 transparent PNG plus its source SVG. Rules:

- GLYPHS come from the real brand: simple-icons (Apache-2.0) where available,
  the official OpenAI mark (simple-icons v9, flower only - the tile path is
  stripped), the official xAI mark cropped from the Wikipedia render of their
  logo, and the Nous Research wordmark from their GitHub org avatar.
- SIZES are standardized: each glyph is tight-cropped, then scaled to fit a
  shared optical box (OPTICAL px inside the 64px canvas) and centered, so no
  logo looks oversized or tiny next to another in the TUI busy-line gutter.
- COLORS follow one rule: keep the saturated brand color when there is one
  (deepseek blue, gemini purple, openai teal, meta blue, googlecloud blue,
  perplexity teal); brands whose mark is near-black get the warm clay
  neutral #E8E4D8 so they stay visible on the dark TUI canvas (same
  treatment anthropic has always had here).

Usage: python scripts/fetch_provider_logos.py  (needs Pillow + svglib)
"""

from __future__ import annotations

import io
import os
import re
import struct
import sys
import urllib.request

from PIL import Image, ImageFilter
from svglib.svglib import svg2rlg
from reportlab.graphics import renderPM

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "assets", "provider-logos")
CANVAS = 64
# Shared optical box: every glyph fits inside this square, centered.
OPTICAL = 56
# Warm clay neutral used for near-black brand marks (dark-canvas safe).
CLAY = (0xE8, 0xE4, 0xD8)

# provider id -> (simple-icons slug, target rgb)
SIMPLE_ICONS = {
    "anthropic": ("anthropic", CLAY),
    "copilot": ("githubcopilot", CLAY),
    "deepseek": ("deepseek", (0x4D, 0x6B, 0xFE)),
    "gemini": ("googlegemini", (0x8E, 0x75, 0xB2)),
    "googlecloud": ("googlecloud", (0x42, 0x85, 0xF4)),
    "kimi": ("kimi", CLAY),
    "meta": ("meta", (0x04, 0x68, 0xD7)),
    "ollama": ("ollama", CLAY),
    "openrouter": ("openrouter", CLAY),
    "perplexity": ("perplexity", (0x20, 0x80, 0x8D)),
    "opencode": ("opencode", CLAY),
}

# Official OpenAI flower (simple-icons v9 kept in-repo; simple-icons later
# removed the slug). We strip the rounded-tile path and keep the knot.
OPENAI_LOCAL_SVG = "openai.svg"
OPENAI_COLOR = (0x74, 0xAA, 0x9C)

XAI_WIKI_URL = (
    "https://commons.wikimedia.org/wiki/Special:FilePath/"
    "Logo%20Grok%20AI%20(xAI)%202025.png"
)
XAI_COLOR = CLAY

NOUS_AVATAR_URL = "https://github.com/NousResearch.png"
NOUS_COLOR = CLAY

# Cline ships its mark as a LIGHT glyph on the brand's dark tile. simple-icons
# carries the same outline (hex 18181B) but its path relies on nonzero winding,
# which svglib cannot be trusted to fill, so the official app icon is the
# source of truth and only the light glyph is kept.
CLINE_ICON_URL = (
    "https://raw.githubusercontent.com/cline/cline/main/"
    "apps/cline-hub/src/webview/public/icon.png"
)
CLINE_COLOR = CLAY


def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "nur-cli-logo-fetch/1.0"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return resp.read()


def black_on_white_from_svg(svg_bytes: bytes, size: int = 512) -> Image.Image:
    """Render an SVG as an opaque white canvas with a flat black glyph."""
    text = svg_bytes.decode("utf-8")
    # Normalize every fill to black so alpha can be derived from luminance.
    text = re.sub(r'fill="[^"]*"', 'fill="#000000"', text)
    text = re.sub(r"fill:'[^']*'", "fill:'#000000'", text)
    drawing = svg2rlg(io.BytesIO(text.encode("utf-8")))
    scale = size / max(drawing.width, drawing.height)
    drawing.scale(scale, scale)
    drawing.width = drawing.height = size
    png = renderPM.drawToString(drawing, fmt="PNG", bg=0xFFFFFF)
    return Image.open(io.BytesIO(png)).convert("L")


def glyph_from_luma(luma: Image.Image, invert: bool = False) -> Image.Image:
    """Alpha mask (L mode) from a black-on-white (or inverted) raster."""
    if invert:
        luma = luma.point(lambda v: 255 - v)
    alpha = luma.point(lambda v: 255 - v)
    # Kill near-transparent halos left by anti-aliased crop edges and
    # compression noise so tiles stay clean on any canvas.
    alpha = alpha.point(lambda v: 0 if v < 24 else v)
    bbox = alpha.getbbox()
    return alpha.crop(bbox) if bbox else alpha


def standardize(mask: Image.Image, color: tuple[int, int, int]) -> Image.Image:
    """Fit a glyph mask into the shared optical box on a 64x64 canvas."""
    scale = OPTICAL / max(mask.width, mask.height)
    w = max(1, round(mask.width * scale))
    h = max(1, round(mask.height * scale))
    mask = mask.resize((w, h), Image.LANCZOS)
    canvas = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    glyph = Image.new("RGBA", (w, h), color + (0,))
    glyph.putalpha(mask)
    x = (CANVAS - w) // 2
    y = (CANVAS - h) // 2
    canvas.alpha_composite(glyph, (x, y))
    return canvas


def save(name: str, img: Image.Image, svg_bytes: bytes | None) -> None:
    img.save(os.path.join(OUT, f"{name}.png"), optimize=True)
    if svg_bytes is not None:
        with open(os.path.join(OUT, f"{name}.svg"), "wb") as fh:
            fh.write(svg_bytes)
    print(f"  {name}.png  {CANVAS}x{CANVAS}")


def do_simple_icons(name: str, slug: str, color) -> None:
    # cdn.simpleicons.org rate-limits/blocks bulk fetches (403), so fall back to
    # the same icon's source SVG in the simple-icons repo (Apache-2.0). Without
    # this a single 403 aborted the whole regeneration run.
    try:
        svg = fetch(f"https://cdn.simpleicons.org/{slug}")
    except Exception:
        svg = fetch(
            "https://raw.githubusercontent.com/simple-icons/"
            f"simple-icons/master/icons/{slug}.svg"
        )
    mask = glyph_from_luma(black_on_white_from_svg(svg))
    save(name, standardize(mask, color), svg)


def do_openai() -> None:
    svg_path = os.path.join(OUT, OPENAI_LOCAL_SVG)
    with open(svg_path, "rb") as fh:
        svg = fh.read()
    # Drop the rounded-tile path (first path, carries the teal fill) and
    # flatten the flower to black for the alpha pass.
    text = svg.decode("utf-8")
    text = re.sub(r'<path d="M1 578\.4[^/]*/>', "", text, flags=re.S)
    mask = glyph_from_luma(black_on_white_from_svg(text.encode("utf-8")))
    save("openai", standardize(mask, OPENAI_COLOR), svg)


def _largest_component_bbox(mask: Image.Image, step: int = 4) -> tuple[int, int, int, int]:
    """Bounding box of the largest white connected component, in original
    coordinates. Flood-fills a step-x downsample for speed."""
    small = mask.resize((mask.width // step, mask.height // step))
    w, h = small.size
    px = small.load()
    seen = [[False] * w for _ in range(h)]
    best: tuple[int, int, int, int, int] = (0, 0, 0, 0, 0)  # area x0 y0 x1 y1
    for sy in range(h):
        for sx in range(w):
            if not px[sx, sy] or seen[sy][sx]:
                continue
            stack = [(sx, sy)]
            seen[sy][sx] = True
            n = 0
            x0 = x1 = sx
            y0 = y1 = sy
            while stack:
                x, y = stack.pop()
                n += 1
                x0, x1 = min(x0, x), max(x1, x)
                y0, y1 = min(y0, y), max(y1, y)
                for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                    if 0 <= nx < w and 0 <= ny < h and px[nx, ny] and not seen[ny][nx]:
                        seen[ny][nx] = True
                        stack.append((nx, ny))
            if n > best[0]:
                best = (n, x0, y0, x1, y1)
    _, x0, y0, x1, y1 = best
    return (x0 * step, y0 * step, (x1 + 1) * step, (y1 + 1) * step)


def do_xai() -> None:
    png = fetch(XAI_WIKI_URL)
    img = Image.open(io.BytesIO(png)).convert("L")
    # The official render is a black glyph inside a solid white square,
    # followed by a "Grok" wordmark on black. The square is the largest
    # connected white region; the wordmark's strokes are thin and lose.
    white = img.point(lambda v: 255 if v > 200 else 0)
    box = _largest_component_bbox(white)
    # Inset past the square's anti-aliased rim so its edge cannot ghost
    # back as a faint frame around the glyph.
    inset = max(2, (box[2] - box[0]) // 100)
    square = img.crop((box[0] + inset, box[1] + inset, box[2] - inset, box[3] - inset))
    # Glyph mask: black mark on the white square.
    mask = glyph_from_luma(square)
    save("xai", standardize(mask, XAI_COLOR), None)


def do_nous() -> None:
    png = fetch(NOUS_AVATAR_URL)
    img = Image.open(io.BytesIO(png)).convert("L")
    # The org avatar is dark lettering on a white fuzzy blob on black. The
    # letters are dark like the background, so keep only dark pixels the
    # background flood cannot reach: flood dark pixels inward from the
    # border, then the un-reached dark region is the enclosed wordmark.
    # Then keep the tallest row-run: the big NOUS word (the tiny RESEARCH
    # line dissolves at 64px anyway).
    dark = img.point(lambda v: 255 if v < 110 else 0)
    w, h = dark.size
    px = dark.load()
    from collections import deque

    reachable = [[False] * w for _ in range(h)]
    queue: deque[tuple[int, int]] = deque()
    for x in range(w):
        for y in (0, h - 1):
            if px[x, y] and not reachable[y][x]:
                reachable[y][x] = True
                queue.append((x, y))
    for y in range(h):
        for x in (0, w - 1):
            if px[x, y] and not reachable[y][x]:
                reachable[y][x] = True
                queue.append((x, y))
    while queue:
        x, y = queue.popleft()
        for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
            if 0 <= nx < w and 0 <= ny < h and px[nx, ny] and not reachable[ny][nx]:
                reachable[ny][nx] = True
                queue.append((nx, ny))
    letters = Image.new("L", (w, h), 0)
    lpx = letters.load()
    for y in range(h):
        for x in range(w):
            if px[x, y] and not reachable[y][x]:
                lpx[x, y] = 255
    # Drop specks: ring fragments of the fuzzy blob survive the flood as
    # small islands. Any component under 2% of the surviving ink goes.
    area = sum(sum(1 for x in range(w) if lpx[x, y]) for y in range(h))
    seen = [[False] * w for _ in range(h)]
    for sy in range(h):
        for sx in range(w):
            if not lpx[sx, sy] or seen[sy][sx]:
                continue
            stack = [(sx, sy)]
            seen[sy][sx] = True
            comp = []
            while stack:
                x, y = stack.pop()
                comp.append((x, y))
                for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                    if 0 <= nx < w and 0 <= ny < h and lpx[nx, ny] and not seen[ny][nx]:
                        seen[ny][nx] = True
                        stack.append((nx, ny))
            if len(comp) < area * 0.02:
                for x, y in comp:
                    lpx[x, y] = 0
    row_has = [any(lpx[x, y] for x in range(0, w, 2)) for y in range(h)]
    runs = []
    start = None
    for y, has in enumerate(row_has + [False]):
        if has and start is None:
            start = y
        elif not has and start is not None:
            runs.append((start, y))
            start = None
    y0, y1 = max(runs, key=lambda r: r[1] - r[0])
    mask = letters.crop((0, y0, w, y1))
    bbox = mask.getbbox()
    mask = mask.crop(bbox)
    save("nous", standardize(mask, NOUS_COLOR), None)


def do_cline() -> None:
    png = fetch(CLINE_ICON_URL)
    img = Image.open(io.BytesIO(png)).convert("RGBA")
    # Flatten onto black: the app icon is a light glyph inside a dark rounded
    # tile, and any transparent margin must not read as glyph.
    flat = Image.new("RGBA", img.size, (0, 0, 0, 255))
    flat = Image.alpha_composite(flat, img).convert("L")
    # Keep the light glyph only - the dark tile background drops out, and the
    # two eye cutouts inside the glyph stay holes because they are dark too.
    mask = flat.point(lambda v: 255 if v > 170 else 0)
    bbox = mask.getbbox()
    if bbox is None:
        raise RuntimeError("cline icon yielded an empty glyph mask")
    save("cline", standardize(mask.crop(bbox), CLINE_COLOR), None)


def main() -> int:
    os.makedirs(OUT, exist_ok=True)
    print("fetching simple-icons…")
    for name, (slug, color) in SIMPLE_ICONS.items():
        do_simple_icons(name, slug, color)
    print("openai (local official svg, tile stripped)…")
    do_openai()
    print("xai (official mark from wikipedia render)…")
    do_xai()
    print("nous (github org wordmark)…")
    do_nous()
    print("cline (official app icon, glyph only)…")
    do_cline()

    # Verify every PNG is exactly 64x64 RGBA before finishing.
    for entry in sorted(os.listdir(OUT)):
        if entry.endswith(".png"):
            with open(os.path.join(OUT, entry), "rb") as fh:
                head = fh.read(24)
            w, h = struct.unpack(">II", head[16:24])
            if (w, h) != (CANVAS, CANVAS):
                print(f"FAIL {entry}: {w}x{h}", file=sys.stderr)
                return 1
    print("all logos standardized at 64x64")
    return 0


if __name__ == "__main__":
    sys.exit(main())
