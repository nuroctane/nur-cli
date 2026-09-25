"""Scripted OpenAI-compatible Chat Completions server for nur's E2E suite.

Each POST /v1/chat/completions pops the next scripted reply and logs the full
request body, so a scenario can assert on what nur actually sent (tool results,
system prompt, message order) as well as on what it did.

A reply is {"text": "..."}, {"tool_calls": [{"name", "arguments"}]} (a string
"arguments" is sent verbatim, so malformed JSON can be scripted), or
{"status": 500, "error": "..."} for an HTTP failure.
When the script runs out the server answers with a fixed text, so a runaway
loop ends instead of hanging; the runner then fails on the request count.

Usage: python fake_provider.py <script.json> <requests.jsonl> <port-file>
"""

import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

EXHAUSTED = "[fake provider: script exhausted]"


class State:
    def __init__(self, script, log_path):
        self.replies = list(script)
        self.log_path = log_path
        self.lock = threading.Lock()
        self.count = 0

    def next_reply(self, body):
        with self.lock:
            self.count += 1
            with open(self.log_path, "a", encoding="utf-8") as log:
                log.write(json.dumps(body) + "\n")
            return self.replies.pop(0) if self.replies else {"text": EXHAUSTED}


def tool_calls_of(reply, n):
    return [
        {
            "index": i,
            "id": f"call_{n}_{i}",
            "type": "function",
            "function": {
                "name": call["name"],
                "arguments": call["arguments"]
                if isinstance(call.get("arguments"), str)
                else json.dumps(call.get("arguments", {})),
            },
        }
        for i, call in enumerate(reply.get("tool_calls", []))
    ]


def make_handler(state):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            # /v1/models probes: answer with the one scripted model.
            self._json(200, {"object": "list", "data": [{"id": "e2e-model", "object": "model"}]})

        def do_POST(self):
            length = int(self.headers.get("content-length", "0"))
            body = json.loads(self.rfile.read(length) or b"{}")
            if not self.path.rstrip("/").endswith("/chat/completions"):
                self._json(404, {"error": {"message": f"unexpected path {self.path}"}})
                return
            reply = state.next_reply(body)
            n = state.count
            if "status" in reply:
                # Scripted HTTP failure: {"status": 500, "error": "..."}.
                self._json(reply["status"], {"error": {"message": reply.get("error", "scripted failure")}})
                return
            calls = tool_calls_of(reply, n)
            text = reply.get("text", "")
            finish = "tool_calls" if calls else "stop"
            usage = {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
            if body.get("stream"):
                self.send_response(200)
                self.send_header("content-type", "text/event-stream")
                self.end_headers()
                delta = {"role": "assistant"}
                if text:
                    delta["content"] = text
                if calls:
                    delta["tool_calls"] = calls
                self._sse({"choices": [{"index": 0, "delta": delta, "finish_reason": None}]}, n)
                self._sse({"choices": [{"index": 0, "delta": {}, "finish_reason": finish}], "usage": usage}, n)
                self.wfile.write(b"data: [DONE]\n\n")
                self.wfile.flush()
                return
            message = {"role": "assistant", "content": text or None}
            if calls:
                message["tool_calls"] = [{k: v for k, v in c.items() if k != "index"} for c in calls]
            self._json(200, {
                "id": f"cmpl-{n}",
                "object": "chat.completion",
                "model": body.get("model", "e2e-model"),
                "choices": [{"index": 0, "message": message, "finish_reason": finish}],
                "usage": usage,
            })

        def _sse(self, chunk, n):
            chunk = {"id": f"cmpl-{n}", "object": "chat.completion.chunk", "model": "e2e-model", **chunk}
            self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
            self.wfile.flush()

        def _json(self, code, payload):
            data = json.dumps(payload).encode()
            self.send_response(code)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    return Handler


def main():
    script_path, log_path, port_file = sys.argv[1:4]
    with open(script_path, encoding="utf-8") as f:
        script = json.load(f)
    server = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(State(script, log_path)))
    with open(port_file, "w", encoding="utf-8") as f:
        f.write(str(server.server_address[1]))
    server.serve_forever()


if __name__ == "__main__":
    main()
