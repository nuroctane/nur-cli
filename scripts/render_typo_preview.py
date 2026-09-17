#!/usr/bin/env python3
"""Render the transcript-typography harness dumps (.tsv) as PNGs.

Companion to the `typography_preview` test in `src/tui/ui.rs`, which writes the
dumps. Design review only - not part of the product.

    NUR_TYPO_DUMP=.nur/typo cargo test --bin nur typography_preview -- --ignored
    py scripts/render_typo_preview.py .nur/typo .nur/typo-png [scale]

Each TSV is one theme at one width; rows are `row \t col \t fg \t bg \t mods \t symbol`.
Only for local design review - not part of the product.
"""
from __future__ import annotations

import os
import sys
import unicodedata
from PIL import Image, ImageDraw, ImageFont

FONT_CANDIDATES = [
    r"C:\Windows\Fonts\consola.ttf",
    r"C:\Windows\Fonts\CascadiaMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
]


def load_font(px: int) -> ImageFont.FreeTypeFont:
    for path in FONT_CANDIDATES:
        if os.path.exists(path):
            return ImageFont.truetype(path, px)
    raise SystemExit("no monospace font found")


def parse_hex(v: str) -> tuple[int, int, int] | None:
    if v == "reset":
        return None
    if v.startswith("#") and len(v) == 7:
        return tuple(int(v[i : i + 2], 16) for i in (1, 3, 5))
    # Named ratatui colors (e.g. Indexed(4)) - approximate.
    named = {
        "Black": (0, 0, 0), "Red": (205, 49, 49), "Green": (13, 188, 121),
        "Yellow": (229, 229, 16), "Blue": (36, 114, 200), "Magenta": (188, 63, 188),
        "Cyan": (17, 168, 205), "Gray": (229, 229, 229), "DarkGray": (102, 102, 102),
        "White": (255, 255, 255),
    }
    for k, rgb in named.items():
        if v.startswith(k):
            return rgb
    return (200, 200, 200)


def render(tsv_path: str, out_path: str, scale: float = 1.0) -> None:
    header = {}
    cells: dict[tuple[int, int], tuple[str, str, str, int]] = {}
    with open(tsv_path, encoding="utf-8") as fh:
        for line in fh:
            if line.startswith("#"):
                for tok in line[1:].split():
                    if "=" in tok:
                        k, v = tok.split("=", 1)
                        header[k] = v
                continue
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 6:
                continue
            y, x, fg, bg, mods, sym = parts[0], parts[1], parts[2], parts[3], parts[4], "\t".join(parts[5:])
            cells[(int(x), int(y))] = (fg, bg, mods, sym)

    width = int(header["width"])
    rows = int(header["rows"])
    theme_bg = parse_hex(header.get("bg", "reset")) or (16, 16, 18)

    px = int(15 * scale)
    font = load_font(px)
    cw = font.getlength("M")
    lh = int(px * 1.32)
    pad = int(8 * scale)
    img = Image.new(
        "RGB", (int(cw * width) + pad * 2, lh * rows + pad * 2), theme_bg
    )
    draw = ImageDraw.Draw(img)

    for (x, y), (fg, bg, mods, sym) in cells.items():
        bg_rgb = parse_hex(bg) or theme_bg
        fg_rgb = parse_hex(fg) or (230, 230, 230)
        if sym.strip() == "" and bg_rgb == theme_bg:
            continue
        wide = unicodedata.east_asian_width(sym[0]) in ("W", "F") if sym else False
        cell_w = cw * (2 if wide else 1)
        x0 = pad + x * cw
        y0 = pad + y * lh
        if bg_rgb != theme_bg:
            draw.rectangle([x0, y0, x0 + cell_w - 1, y0 + lh - 1], fill=bg_rgb)
        if sym.strip():
            bold = bool(int(mods) & 1) if mods.isdigit() else False
            draw.text((x0, y0), sym, font=font, fill=fg_rgb)
    img.save(out_path)
    print(f"{out_path}  {img.size[0]}x{img.size[1]}")


def main() -> int:
    dump, out = sys.argv[1], sys.argv[2]
    scale = float(sys.argv[3]) if len(sys.argv) > 3 else 1.0
    os.makedirs(out, exist_ok=True)
    for name in sorted(os.listdir(dump)):
        if name.endswith(".tsv"):
            render(os.path.join(dump, name), os.path.join(out, name[:-4] + ".png"), scale)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
