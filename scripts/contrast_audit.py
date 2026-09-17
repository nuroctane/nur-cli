#!/usr/bin/env python3
"""WCAG contrast audit over the transcript-typography harness dumps.

Companion to the `typography_preview` test in `src/tui/ui.rs`. Reports every
rendered cell whose foreground/background contrast is under `floor`, so theme
changes can be checked numerically instead of by eye.

    NUR_TYPO_DUMP=.nur/typo cargo test --bin nur typography_preview -- --ignored
    py scripts/contrast_audit.py .nur/typo 3.0

For every rendered cell it resolves the effective foreground (falling back to
the theme's fg) and background (falling back to the theme's bg) and reports the
worst offenders per theme. Design review only - not part of the product.
"""
from __future__ import annotations

import os
import sys


def parse(v):
    if not v or v == "reset":
        return None
    if v.startswith("#") and len(v) == 7:
        return tuple(int(v[i : i + 2], 16) for i in (1, 3, 5))
    named = {
        "Black": (0, 0, 0), "Red": (205, 49, 49), "Green": (13, 188, 121),
        "Yellow": (229, 229, 16), "Blue": (36, 114, 200), "Magenta": (188, 63, 188),
        "Cyan": (17, 168, 205), "Gray": (229, 229, 229), "DarkGray": (102, 102, 102),
        "White": (255, 255, 255),
    }
    for k, rgb in named.items():
        if v.startswith(k):
            return rgb
    return None


def lum(rgb):
    def ch(c):
        c /= 255.0
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    r, g, b = (ch(c) for c in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    la, lb = lum(a), lum(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


def audit(path):
    header = {}
    cells = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            if line.startswith("#"):
                for tok in line[1:].split():
                    if "=" in tok:
                        k, v = tok.split("=", 1)
                        header[k] = v
                continue
            p = line.rstrip("\n").split("\t")
            if len(p) < 6:
                continue
            cells.append((int(p[0]), int(p[1]), p[2], p[3], "\t".join(p[5:])))
    theme_bg = parse(header.get("bg", "")) or (16, 16, 18)
    theme_fg = parse(header.get("fg", "")) or (230, 230, 230)
    rowtext = {}
    for y, _x, _fg, _bg, sym in cells:
        rowtext.setdefault(y, []).append(sym)
    worst = []
    for y, _x, fg, bg, sym in cells:
        if not sym.strip():
            continue
        f = parse(fg) or theme_fg
        b = parse(bg) or theme_bg
        ratio = contrast(f, b)
        if ratio < 4.5:
            row = "".join(rowtext.get(y, [])).strip()
            worst.append((ratio, y, f"fg={fg} bg={bg} | {row[:52]}"))
    worst.sort()
    return header.get("theme", "?"), worst


def main():
    dump = sys.argv[1]
    floor = float(sys.argv[2]) if len(sys.argv) > 2 else 3.0
    out = []
    any_bad = False
    for name in sorted(os.listdir(dump)):
        if not name.endswith(".tsv"):
            continue
        theme, worst = audit(os.path.join(dump, name))
        below = [w for w in worst if w[0] < floor]
        if below:
            any_bad = True
        head = f"{name:22} worst={worst[0][0]:.2f}" if worst else f"{name:22} worst=n/a"
        out.append(head + ("   <-- BELOW FLOOR" if below else ""))
        for ratio, y, snippet in worst[:3]:
            out.append(f"    {ratio:5.2f}  row {y:3}  {snippet}")
    sys.stdout.buffer.write(("\n".join(out) + "\n").encode("utf-8"))
    return 1 if any_bad else 0


if __name__ == "__main__":
    raise SystemExit(main())
