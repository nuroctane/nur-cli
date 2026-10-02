"""Record the NurCLI demo take: drive the real TUI and save an asciicast.

Everything on screen is the real binary in a real Windows terminal (ConPTY).
A scripted Responses server stands in for the model so every take is
identical, and a small Python project with a real precedence bug gives the
tools real work: the failing run, the reads, the diff and the passing run are
genuine tool output.

Run: python scripts/demo/record.py [--bin target/release/nur.exe] [--root C:\\code]
Writes target/demo/take.cast (asciicast v2) and target/demo/take.json: scene
markers, key presses and mouse clicks, which the player animates.
"""
import argparse
import json
import pathlib
import queue
import random
import shutil
import subprocess
import sys
import tempfile
import threading
import time

import pyte

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tests" / "e2e"))
from run_e2e import isolated_env  # noqa: E402

OUT = ROOT / "target" / "demo"
COLS, ROWS = 124, 40
# With /sidegraph open the panel takes a third of the width (src/tui/ui.rs).
TRANSCRIPT, SIDEGRAPH = (0, COLS - COLS // 3), (COLS - COLS // 3, COLS)
# Where the graph scene drags the panel's left border: wide enough for the
# main column and its tool tracks.
WIDE_GRAPH_LEFT = 52
ENTER, ESC, BACKSPACE = "\r", "\x1b", "\x7f"
UP, DOWN, F6 = "\x1b[A", "\x1b[B", "\x1b[17~"
KEY_LABELS = {ENTER: "↵", ESC: "esc", BACKSPACE: "⌫", UP: "↑", DOWN: "↓", F6: "F6",
              "\x1b2": "⌥ 2", "\x1b4": "⌥ 4"}

PARSER_BUG = '''def evaluate(source):
    tokens = tokenize(source)
    result = tokens[0]
    for i in range(1, len(tokens), 2):
        result = apply(result, tokens[i], tokens[i + 1])
    return result
'''
PARSER_FIX = '''def evaluate(source):
    tokens = tokenize(source)
    # Fold * and / into their terms first, then apply + and -.
    terms = [tokens[0]]
    for op, value in zip(tokens[1::2], tokens[2::2]):
        if op in "*/":
            terms[-1] = apply(terms[-1], op, value)
        else:
            terms += [op, value]
    result = terms[0]
    for op, value in zip(terms[1::2], terms[2::2]):
        result = apply(result, op, value)
    return result
'''
GARBAGE_TEST = '''    def test_rejects_garbage(self):
        with self.assertRaises(ValueError):
            evaluate("2 + banana")
'''
CHAINED_TEST = '''
    def test_chained_division(self):
        self.assertEqual(evaluate("8 / 4 / 2"), 1)
'''
PROJECT = {
    "calc/__init__.py": "from .parser import evaluate\n",
    "calc/parser.py": '''"""Tiny arithmetic evaluator behind the calc command."""

import re

TOKEN = re.compile(r"\\s*(\\d+(?:\\.\\d+)?|[+\\-*/])")


def tokenize(source):
    tokens = TOKEN.findall(source)
    if "".join(tokens) != source.replace(" ", ""):
        raise ValueError(f"cannot parse {source!r}")
    return [float(t) if t[0].isdigit() else t for t in tokens]


def apply(left, op, right):
    if op == "+":
        return left + right
    if op == "-":
        return left - right
    if op == "*":
        return left * right
    return left / right


''' + PARSER_BUG,
    "calc/__main__.py": '''import sys

from . import evaluate

print(evaluate(" ".join(sys.argv[1:])))
''',
    "tests/__init__.py": "",
    "tests/test_parser.py": '''import unittest

from calc import evaluate


class EvaluateTests(unittest.TestCase):
    def test_addition(self):
        self.assertEqual(evaluate("1 + 2"), 3)

    def test_subtraction(self):
        self.assertEqual(evaluate("9 - 4 - 1"), 4)

    def test_multiplication(self):
        self.assertEqual(evaluate("6 * 7"), 42)

    def test_precedence(self):
        self.assertEqual(evaluate("2 + 3 * 4"), 14)

    def test_division_binds_tighter(self):
        self.assertEqual(evaluate("10 - 6 / 2"), 7)

    def test_rejects_garbage(self):
        with self.assertRaises(ValueError):
            evaluate("2 + banana")


if __name__ == "__main__":
    unittest.main()
''',
    "README.md": "# calc\n\nA tiny arithmetic evaluator: `python -m calc 2 + 3 * 4`.\n",
}

PROMPT = "the precedence tests in calc are failing. find the bug, fix it, and prove it with the suite"
SCRIPT = [
    {"reasoning": "Reproduce first: run the suite and read exactly what breaks.",
     "tool_calls": [{"name": "bash", "arguments": {"command": "python -m unittest"}}],
     "usage": {"input_tokens": 11840, "output_tokens": 96}},
    {"reasoning": "2 + 3 * 4 gives 20 and 10 - 6 / 2 gives 2, so evaluation runs strictly left to "
                  "right. Read the evaluator, the tests and every caller together.",
     "tool_calls": [{"name": "read_file", "arguments": {"path": "calc/parser.py"}},
                    {"name": "read_file", "arguments": {"path": "tests/test_parser.py"}},
                    {"name": "grep", "arguments": {"pattern": "evaluate\\(", "path": "."}}],
     "usage": {"input_tokens": 12610, "output_tokens": 141}},
    {"reasoning": "evaluate() folds every operator in one pass. Fold * and / into their terms first, "
                  "then apply + and -. The public API stays the same.",
     "tool_calls": [{"name": "edit_file", "arguments": {"path": "calc/parser.py",
                                                         "old_string": PARSER_BUG, "new_string": PARSER_FIX}}],
     "usage": {"input_tokens": 14020, "output_tokens": 312}},
    {"reasoning": "Prove it with the same suite.",
     "tool_calls": [{"name": "bash", "arguments": {"command": "python -m unittest -v"}}],
     "usage": {"input_tokens": 14780, "output_tokens": 64}},
    {"text": "## Fixed operator precedence\n\n`evaluate()` folded every operator left to right, so "
             "`2 + 3 * 4` came out as **20**. It now folds `*` and `/` into their terms first, then "
             "applies `+` and `-`:\n\n"
             "- `calc/parser.py`: two-pass evaluation, same public API\n"
             "- all **6 tests** pass, including `test_precedence`\n\n"
             "```text\n2 + 3 * 4   ->  14\n10 - 6 / 2  ->  7\n```\n",
     "usage": {"input_tokens": 15660, "output_tokens": 186}},
    # The follow-up, filmed in a second theme.
    {"reasoning": "Pin chained division too: 8 / 4 / 2 must fold left to right inside its term.",
     "tool_calls": [{"name": "edit_file", "arguments": {"path": "tests/test_parser.py",
                                                         "old_string": GARBAGE_TEST,
                                                         "new_string": GARBAGE_TEST + CHAINED_TEST}}],
     "usage": {"input_tokens": 16420, "output_tokens": 118}},
    {"reasoning": "Run the whole suite again.",
     "tool_calls": [{"name": "bash", "arguments": {"command": "python -m unittest -v"}}],
     "usage": {"input_tokens": 17150, "output_tokens": 52}},
    {"text": "Added `test_chained_division`: `8 / 4 / 2` evaluates to **1**. All **7 tests** pass.\n",
     "usage": {"input_tokens": 17890, "output_tokens": 61}},
]
PROMPT_2 = "add a regression test for chained division, 8 / 4 / 2, and run the suite"
# /theme steps from gold to the theme the follow-up is filmed in (synthwave).
SECOND_THEME_STEPS = 12


class Recorder:
    """A real terminal session whose output is timestamped for asciicast."""

    def __init__(self, binary, cwd, env):
        from conpty import ConPtyProcess
        self.screen = pyte.Screen(COLS, ROWS)
        self.stream = pyte.Stream(self.screen)
        self.events, self.scenes, self.keys_log, self.clicks = [], [], [], []
        self.chunks = queue.Queue()
        self.start = time.monotonic()
        self.proc = ConPtyProcess(binary, cwd, env, columns=COLS, rows=ROWS)
        threading.Thread(target=self._read, daemon=True).start()

    def now(self):
        return round(time.monotonic() - self.start, 3)

    def _read(self):
        try:
            while True:
                chunk = self.proc.read()
                if not chunk:
                    return
                self.chunks.put((time.monotonic() - self.start, chunk))
        except (OSError, EOFError):
            pass

    def feed(self, timeout):
        try:
            at, chunk = self.chunks.get(timeout=timeout)
        except queue.Empty:
            return False
        self.events.append([round(at, 4), "o", chunk])
        self.stream.feed(chunk)
        return True

    def text(self):
        return "\n".join(self.screen.display)

    def until(self, needle, timeout=20):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if needle in self.text():
                return
            self.feed(0.03)
        raise AssertionError(f"screen never showed {needle!r}:\n{self.text()}")

    def pump(self, seconds):
        deadline = time.monotonic() + seconds
        while (left := deadline - time.monotonic()) > 0:
            self.feed(min(0.03, left))

    def scene(self, name):
        self.scenes.append({"name": name, "t": self.now()})

    def find(self, needle, cols=(0, COLS)):
        """First (column, row) where needle starts inside the column range."""
        lo, hi = cols
        for y, row in enumerate(self.screen.display):
            x = row.find(needle, lo, hi)
            if x >= 0:
                return x, y
        raise AssertionError(f"no row shows {needle!r} in columns {cols}:\n{self.text()}")

    def click(self, x, y, button="left"):
        """Click a cell (SGR mouse, as a terminal sends it)."""
        code = {"left": 0, "right": 2}[button]
        self.clicks.append({"t": self.now(), "x": x, "y": y, "button": button})
        self.proc.write(f"\x1b[<{code};{x + 1};{y + 1}M")
        self.pump(0.07)
        self.proc.write(f"\x1b[<{code};{x + 1};{y + 1}m")

    def drag(self, x0, y0, x1, y1, seconds=1.2, steps=24):
        """Press, move with the left button held, release (SGR, as a terminal sends it)."""
        self.clicks.append({"t": self.now(), "x": x0, "y": y0, "button": "press"})
        self.proc.write(f"\x1b[<0;{x0 + 1};{y0 + 1}M")
        self.pump(0.12)
        for i in range(1, steps + 1):
            x = round(x0 + (x1 - x0) * i / steps)
            y = round(y0 + (y1 - y0) * i / steps)
            self.clicks.append({"t": self.now(), "x": x, "y": y, "button": "move"})
            self.proc.write(f"\x1b[<32;{x + 1};{y + 1}M")
            self.pump(seconds / steps)
        self.clicks.append({"t": self.now(), "x": x1, "y": y1, "button": "release"})
        self.proc.write(f"\x1b[<0;{x1 + 1};{y1 + 1}m")
        self.pump(0.1)

    def wheel(self, x, y, up, ctrl=False, times=1, pause=0.35, hint=None):
        """Scroll the wheel over a cell; with ctrl it zooms the sidegraph."""
        code = (64 if up else 65) + (16 if ctrl else 0)
        for _ in range(times):
            t = self.now()
            self.clicks.append({"t": t, "x": x, "y": y, "button": "wheel"})
            if hint:
                label = ("⌃ " if ctrl else "") + ("scroll ↑" if up else "scroll ↓")
                self.keys_log.append({"t": t, "label": label, "hint": hint})
            self.proc.write(f"\x1b[<{code};{x + 1};{y + 1}M")
            self.pump(pause)

    def type(self, text, pace=(0.035, 0.07)):
        for ch in text:
            self.proc.write(ch)
            self.pump(random.uniform(*pace))

    def key(self, key, times=1, pause=0.4, hint=None):
        for _ in range(times):
            self.keys_log.append({"t": self.now(), "label": KEY_LABELS.get(key, key), "hint": hint})
            self.proc.write(key)
            self.pump(pause)

    def close(self):
        self.proc.terminate(force=True)
        self.proc.close(force=True)


def start_provider(work):
    (work / "script.json").write_text(json.dumps(SCRIPT), encoding="utf-8")
    port_file = work / "port"
    log = (OUT / "provider.log").open("w", encoding="utf-8")
    proc = subprocess.Popen([sys.executable, str(ROOT / "scripts" / "demo" / "demo_provider.py"),
                             str(work / "script.json"), str(port_file)],
                            stdout=log, stderr=subprocess.STDOUT)
    for _ in range(200):
        if port_file.exists() and port_file.read_text().strip():
            return proc, int(port_file.read_text())
        time.sleep(0.05)
    proc.kill()
    raise RuntimeError("demo provider did not start")


def perform(rec):
    rec.scene("launch")
    rec.until("F6 inspect", 30)
    rec.pump(2.6)

    # The live execution graph rides beside the transcript for the whole turn.
    rec.scene("sidegraph")
    # Far enough that the palette narrows to the one command.
    rec.type("/sidegr", pace=(0.08, 0.12))
    rec.pump(0.9)
    rec.type("aph", pace=(0.05, 0.08))
    rec.key(ENTER, pause=0.1)
    rec.until("sidegraph", 10)
    rec.pump(1.2)

    rec.scene("prompt")
    rec.type(PROMPT)
    rec.pump(0.6)
    rec.key(ENTER, pause=0.1)

    rec.scene("turn")
    # Manual mode is the default: each kind of side effect asks first.
    rec.until("approve · bash", 60)
    rec.scene("approve-bash")
    rec.pump(1.7)
    rec.key("a", pause=0.1, hint="always")
    rec.scene("reads")
    rec.until("approve · edit_file", 60)
    rec.scene("approve-edit")
    rec.pump(3.0)
    rec.key("y", pause=0.1, hint="once")
    rec.scene("verify")
    rec.until("->  7", 90)
    rec.scene("answer")
    rec.pump(2.4)

    rec.scene("peek")
    x, y = rec.find("edit_file", TRANSCRIPT)
    rec.click(rec.screen.display[y].rindex("▸", 0, x), y)
    rec.pump(3.0)

    # Drag the graph's border open so its tool tracks fit beside the main
    # column, then scroll back through the whole run and return to live.
    rec.scene("graph")
    border = rec.find("◈ sidegraph", SIDEGRAPH)[0] - 2
    rec.pump(0.4)
    rec.drag(border, ROWS // 2, WIDE_GRAPH_LEFT, ROWS // 2, seconds=1.4)
    rec.pump(1.6)
    x, y = (WIDE_GRAPH_LEFT + COLS) // 2, ROWS // 2
    rec.wheel(x, y, up=True, times=10, pause=0.26, hint="history")
    rec.pump(1.2)
    rec.wheel(x, y, up=False, times=10, pause=0.1, hint="live")
    rec.pump(0.8)

    rec.scene("palette")
    rec.type("/", pace=(0.05, 0.05))
    rec.pump(1.3)
    rec.type("login", pace=(0.09, 0.13))
    rec.pump(0.5)
    rec.key(ENTER, pause=0.1)

    rec.scene("providers")
    rec.until("choose a provider", 10)
    rec.pump(1.4)
    rec.type("gem", pace=(0.2, 0.26))
    rec.pump(1.3)
    rec.key(BACKSPACE, 3, 0.14)
    rec.pump(0.3)
    # Hold the arrow down through the whole catalogue.
    rec.key(DOWN, 46, 0.075)
    rec.pump(1.0)
    rec.key(ESC, pause=0.7)

    rec.scene("models")
    rec.type("/model", pace=(0.06, 0.1))
    rec.key(ENTER, pause=0.1)
    rec.until("gpt-6-astra", 15)
    rec.pump(1.1)
    rec.type("astra", pace=(0.14, 0.2))
    rec.pump(0.8)
    rec.key(ENTER, pause=0.1)
    rec.until("model → gpt-6-astra", 5)
    rec.pump(1.3)

    # Browse the themes, then keep one: the follow-up runs in it.
    rec.scene("themes")
    rec.type("/theme", pace=(0.06, 0.1))
    rec.key(ENTER, pause=0.1)
    rec.until("Live preview", 10)
    rec.pump(1.0)
    rec.key(DOWN, SECOND_THEME_STEPS, 0.6)
    rec.pump(0.8)
    rec.key(ENTER, pause=1.2, hint="keep")

    rec.scene("second")
    rec.type(PROMPT_2)
    rec.pump(0.5)
    rec.key(ENTER, pause=0.1)
    rec.scene("second-turn")
    rec.until("approve · edit_file", 60)
    rec.scene("approve-test")
    rec.pump(2.0)
    rec.key("y", pause=0.1, hint="once")
    rec.until("All 7 tests pass", 60)
    rec.scene("second-answer")
    rec.pump(2.6)

    rec.scene("inspector")
    rec.key(F6, pause=1.6)
    rec.key("\x1b2", pause=2.0)
    rec.key(F6, pause=1.2)
    rec.scene("end")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", default=str(ROOT / "target" / "release" / "nur.exe"))
    parser.add_argument("--root", default="C:\\code",
                        help="the project lives at <root>\\calc; a short path keeps the banner tidy")
    args = parser.parse_args()
    random.seed(7)
    OUT.mkdir(parents=True, exist_ok=True)
    root = pathlib.Path(args.root)
    created_root = not root.exists()
    workspace = root / "calc"
    if workspace.exists():
        sys.exit(f"{workspace} already exists; pass --root to record somewhere else")
    work = pathlib.Path(tempfile.mkdtemp(prefix="nur-demo-"))
    for rel, text in PROJECT.items():
        (workspace / rel).parent.mkdir(parents=True, exist_ok=True)
        (workspace / rel).write_text(text, encoding="utf-8", newline="\n")
    provider, port = start_provider(work)
    env = isolated_env(work / "home", port)
    for noisy in ("NUR_PRICING_OFF", "NUR_MODELS_DEV_OFF", "NO_COLOR"):
        env.pop(noisy, None)
    env.update({"OPENAI_API_KEY": "sk-demo-recording-0000000000000000", "TERM": "xterm-256color",
                "COLORTERM": "truecolor", "NUR_DISABLE_NATIVE_MEMORY": "1"})
    home = pathlib.Path(env["NUR_HOME"])
    (home / "config.toml").write_text(
        'provider = "openai"\n'
        f'base_url = "http://127.0.0.1:{port}/v1"\n'
        'model = "gpt-6.1-sol"\ntheme = "gold"\nauto_update = false\n'
        'ecosystem_auto_ensure = false\nnative_memory = false\n\n'
        '[typesafe]\nenabled = false\n', encoding="utf-8")
    # A first launch indexes every installed skill; do that off camera.
    subprocess.run([args.bin, "ecosystem", "refresh-skills"], cwd=workspace, env=env,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True, timeout=300)
    rec = Recorder(pathlib.Path(args.bin), workspace, env)
    # A take that fails part way is kept as failed-take.* (snapshot.py --take
    # failed-take shows where it went wrong) and never replaces a good take.
    name = "failed-take"
    try:
        perform(rec)
        name = "take"
    finally:
        rec.close()
        provider.kill()
        shutil.rmtree(workspace, ignore_errors=True)
        if created_root:
            shutil.rmtree(root, ignore_errors=True)
        shutil.rmtree(work, ignore_errors=True)
        save(rec, name)


def save(rec, name):
    header = {"version": 2, "width": COLS, "height": ROWS, "timestamp": int(time.time()),
              "title": "nur", "env": {"TERM": "xterm-256color", "SHELL": "nur"}}
    cast = OUT / f"{name}.cast"
    with cast.open("w", encoding="utf-8", newline="\n") as f:
        f.write(json.dumps(header) + "\n")
        for event in rec.events:
            f.write(json.dumps(event, ensure_ascii=False) + "\n")
    take = {"cols": COLS, "rows": ROWS, "duration": rec.now(), "scenes": rec.scenes,
            "keys": rec.keys_log, "clicks": rec.clicks}
    (OUT / f"{name}.json").write_text(json.dumps(take, indent=1, ensure_ascii=False), encoding="utf-8")
    print(f"wrote {cast} ({len(rec.events)} events, {rec.now():.1f}s)")


if __name__ == "__main__":
    main()
