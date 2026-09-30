"""Exercise the inline helper against a protected-recent-message API fixture.
Failure cases: unstructured tool bodies, wrong selected message, recent-message
protection making inline compression a no-op, and missing read protection names.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HELPER = Path(__file__).resolve().parent.parent / "scripts/headroom_compress.py"
FIXTURE = '''
import json, os
from types import SimpleNamespace
__version__ = "fixture"
def compress(messages, model=None, protect_recent=4):
    call = messages[0]["tool_calls"][0]
    tool = messages[-1]
    assert call["id"] == tool["tool_call_id"]
    name = call["function"]["name"]
    original = tool["content"]
    items = json.loads(original)
    protected = protect_recent > 0 or (os.environ.get("HEADROOM_PROTECT_READS") == "1" and name == "read_file")
    content = original if protected else json.dumps([items[0], items[-1]])
    output = [messages[0], {**tool, "content":content}]
    return SimpleNamespace(messages=output, tokens_saved=len(original)-len(content), compression_ratio=0.0)
'''


class HelperTests(unittest.TestCase):
    def run_helper(self, label, protected=False):
        with tempfile.TemporaryDirectory() as temporary:
            Path(temporary, "headroom.py").write_text(FIXTURE)
            contents = json.dumps([{"id":i,"value":"unchanged"} for i in range(20)])
            result = subprocess.run([sys.executable, str(HELPER), "--json-out", "--label", label],
                                    input=contents, text=True, capture_output=True, timeout=15,
                                    env={**os.environ, "PYTHONPATH":temporary,
                                         "HEADROOM_PROTECT_READS":"1" if protected else "0"})
            self.assertEqual(result.returncode, 0, result.stderr)
            return contents, json.loads(result.stdout)

    def test_inline_result_compresses_and_returns_the_tool_message(self):
        _, result = self.run_helper("browser")
        self.assertGreater(result["tokens_saved"], 0)
        self.assertEqual([item["id"] for item in json.loads(result["content"])], [0,19])

    def test_opted_in_file_read_protection_preserves_exact_body(self):
        contents, result = self.run_helper("read_file", True)
        self.assertEqual(result["content"], contents)
        self.assertEqual(result["tokens_saved"], 0)


if __name__ == "__main__":
    unittest.main()
