"""Scripted OpenAI Responses server for the NurCLI demo recording.

Streams like a live model: reasoning summary and answer text arrive word by
word, tool-call arguments in pieces, with realistic usage on completion. It
also lists a current model catalog for /model.

Usage: python demo_provider.py <script.json> <port-file>
A reply is {"reasoning": str, "text": str, "tool_calls": [{"name", "arguments"}],
"usage": {"input_tokens", "output_tokens"}}; any key may be omitted.
"""
import json
import pathlib
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODELS = [
    "gpt-6.1-sol", "gpt-6-astra", "gpt-6-astra-ultrafast", "gpt-6-luna", "gpt-6-sol",
    "gpt-5.6-cyber", "gpt-image-2.5-sunburst", "gpt-realtime-2.1",
]
WORD_DELAY = 0.022


class Script:
    def __init__(self, replies):
        self.replies = list(replies)
        self.lock = threading.Lock()
        self.count = 0

    def next(self):
        with self.lock:
            self.count += 1
            return self.count, (self.replies.pop(0) if self.replies else {"text": "Done."})


def words(text):
    out, current = [], ""
    for ch in text:
        current += ch
        if ch in " \n":
            out.append(current)
            current = ""
    if current:
        out.append(current)
    return out


def make_handler(script):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *_):
            pass

        def _json(self, code, payload, headers=None):
            data = json.dumps(payload).encode()
            self.send_response(code)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(data)))
            for name, value in (headers or {}).items():
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            if self.path.rstrip("/").endswith("/models"):
                self._json(200, {"object": "list", "data": [
                    {"id": m, "object": "model", "owned_by": "openai"} for m in MODELS]})
            else:
                self._json(404, {"error": {"message": "not found"}})

        def do_POST(self):
            length = int(self.headers.get("content-length", "0"))
            body = json.loads(self.rfile.read(length) or b"{}")
            if not self.path.rstrip("/").endswith("/responses"):
                self._json(404, {"error": {"message": f"unexpected path {self.path}"}})
                return
            n, reply = script.next()
            self._stream(body, reply, n)

        def _event(self, kind, payload):
            data = f"event: {kind}\ndata: {json.dumps(payload)}\n\n".encode()
            self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
            self.wfile.flush()

        def _stream(self, body, reply, n):
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("transfer-encoding", "chunked")
            self.end_headers()
            model = body.get("model", "gpt-6.1-sol")
            base = {"id": f"resp-{n}", "object": "response", "model": model}
            self._event("response.created", {"type": "response.created",
                                             "response": {**base, "status": "in_progress", "output": []}})
            output = []
            index = 0
            reasoning = reply.get("reasoning", "")
            if reasoning:
                for piece in words(reasoning):
                    self._event("response.reasoning_summary_text.delta", {
                        "type": "response.reasoning_summary_text.delta", "output_index": index,
                        "item_id": f"rs-{n}", "summary_index": 0, "delta": piece})
                    time.sleep(WORD_DELAY)
                index += 1
            text = reply.get("text", "")
            if text:
                for piece in words(text):
                    self._event("response.output_text.delta", {
                        "type": "response.output_text.delta", "output_index": index,
                        "item_id": f"msg-{n}", "content_index": 0, "delta": piece})
                    time.sleep(WORD_DELAY)
                output.append({"type": "message", "id": f"msg-{n}", "role": "assistant",
                               "status": "completed",
                               "content": [{"type": "output_text", "text": text, "annotations": []}]})
                index += 1
            for i, call in enumerate(reply.get("tool_calls", [])):
                args = call["arguments"] if isinstance(call["arguments"], str) else json.dumps(call["arguments"])
                item = {"type": "function_call", "id": f"fc-{n}-{i}", "call_id": f"call_{n}_{i}",
                        "name": call["name"], "arguments": args, "status": "completed"}
                for k in range(0, len(args), 24):
                    self._event("response.function_call_arguments.delta", {
                        "type": "response.function_call_arguments.delta", "output_index": index,
                        "item_id": item["id"], "delta": args[k:k + 24]})
                    time.sleep(0.006)
                self._event("response.output_item.done", {"type": "response.output_item.done",
                                                          "output_index": index, "item": item})
                output.append(item)
                index += 1
            usage = reply.get("usage", {"input_tokens": 9800, "output_tokens": 240})
            usage = {**usage, "total_tokens": usage["input_tokens"] + usage["output_tokens"]}
            self._event("response.completed", {"type": "response.completed",
                                               "response": {**base, "status": "completed",
                                                            "output": output, "usage": usage}})
            self.wfile.write(b"0\r\n\r\n")
            self.wfile.flush()

    return Handler


def main():
    script_path, port_file = sys.argv[1:3]
    script = Script(json.loads(pathlib.Path(script_path).read_text(encoding="utf-8")))
    server = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(script))
    pathlib.Path(port_file).write_text(str(server.server_address[1]), encoding="utf-8")
    server.serve_forever()


if __name__ == "__main__":
    main()
