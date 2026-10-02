"""Print the recorded screen at given times or scene markers (for checking a take).

Run: python scripts/demo/snapshot.py [--take failed-take] <marker-or-seconds> [...]
A marker name may take an offset: diff+2.5 is 2.5 s after the diff marker;
"end" is the last frame.
"""
import json
import pathlib
import re
import sys

import pyte

OUT = pathlib.Path(__file__).resolve().parents[2] / "target" / "demo"
args = sys.argv[1:]
take = "take"
if args[:1] == ["--take"]:
    take, args = args[1], args[2:]
lines = (OUT / f"{take}.cast").read_text(encoding="utf-8").splitlines()
header = json.loads(lines[0])
events = [json.loads(line) for line in lines[1:]]
markers = {s["name"]: s["t"] for s in json.loads((OUT / f"{take}.json").read_text(encoding="utf-8"))["scenes"]}

for spec in args:
    match = re.fullmatch(r"([a-z-]+?)?([+-][0-9.]+)?", spec)
    if spec == "end":
        at = events[-1][0]
    elif match and match.group(1):
        at = markers[match.group(1)] + float(match.group(2) or 0)
    else:
        at = float(spec)
    screen = pyte.Screen(header["width"], header["height"])
    stream = pyte.Stream(screen)
    for t, _, data in events:
        if t > at:
            break
        stream.feed(data)
    print(f"===== {spec} (t={at:.2f}s) =====")
    for row in screen.display:
        print(row.rstrip())
