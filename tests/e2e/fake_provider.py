"""Scripted OpenAI-compatible Chat Completions server for nur's E2E suite.

Each POST /v1/chat/completions pops the next scripted reply and logs the full
request body, so a scenario can assert on what nur actually sent (tool results,
system prompt, message order) as well as on what it did.

A reply is {"text": "..."}, {"tool_calls": [{"name", "arguments"}]} (a string
"arguments" is sent verbatim, so malformed JSON can be scripted), or
{"status": 500, "error": "..."} for an HTTP failure. On the Responses wire a
reply may also carry "reasoning" (an encrypted reasoning item, as OpenAI returns
with store=false) and "extra_output" (raw output items such as Perplexity's
search_results), both emitted before the message.
When the script runs out the server answers with a fixed text, so a runaway
loop ends instead of hanging; the runner then fails on the request count.

With an MCP spec it also serves a Streamable HTTP MCP server (the Enclave
stand-in) on the same port; see FakeMcp.

Usage: python fake_provider.py <script.json> <requests.jsonl> <port-file> [<mcp.json> <mcp.jsonl>]
"""

import json
import sys
import threading
import time
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
                log.write(json.dumps({**body, "_received_at_seconds": time.monotonic()}) + "\n")
            if self.replies and self.replies[0].get("max_envelope_chars"):
                ceiling = self.replies[0]["max_envelope_chars"]
                reserve = body.get("max_output_tokens", body.get("max_tokens", body.get("max_completion_tokens",0))) or 0
                if len(json.dumps(body,ensure_ascii=False)) + reserve*4 > ceiling:
                    return {"status":400,"error":"context window exceeded: request and output reservation are too large"}
            return self.replies.pop(0) if self.replies else {"text": EXHAUSTED}


class FakeMcp:
    """Scripted Streamable HTTP MCP server enforcing what a client must send.

    Spec keys: key (required bearer), path (default /mcp), tools, results
    (tool name -> CallToolResult), page_size (tools/list pagination), sse
    (stream tools/call with a progress notification and a server ping first),
    instructions, forget_session_on (answer the first such method with 404 so
    the client must open a new session). Every request is logged.
    """

    def __init__(self, spec, log_path):
        self.spec = spec
        self.path = spec.get("path", "/mcp").rstrip("/")
        self.log_path = log_path
        self.lock = threading.Lock()
        self.sessions = set()
        self.opened = 0
        self.forgot = False

    def handle(self, handler, body):
        headers = handler.headers
        method = body.get("method") if isinstance(body, dict) else None
        with self.lock, open(self.log_path, "a", encoding="utf-8") as log:
            log.write(json.dumps({
                "method": method or "(response)",
                "auth": headers.get("authorization", ""),
                "session": headers.get("mcp-session-id", ""),
                "protocol": headers.get("mcp-protocol-version", ""),
                "params": body.get("params") if isinstance(body, dict) else None,
            }) + "\n")
        if headers.get("authorization") != "Bearer " + self.spec["key"]:
            handler._json(401, {"error": "Unauthorized", "message": "Missing or invalid Authorization header"},
                          {"WWW-Authenticate": 'Bearer resource_metadata="/.well-known/oauth-protected-resource"'})
            return
        if method == "initialize":
            with self.lock:
                self.opened += 1
                session = f"fake-session-{self.opened}"
                self.sessions.add(session)
            result = {"protocolVersion": body["params"]["protocolVersion"], "capabilities": {"tools": {}},
                      "serverInfo": {"name": "fake-enclave", "version": "0.0.1"}}
            if self.spec.get("instructions"):
                result["instructions"] = self.spec["instructions"]
            handler._json(200, {"jsonrpc": "2.0", "id": body["id"], "result": result}, {"Mcp-Session-Id": session})
            return
        session = headers.get("mcp-session-id", "")
        if session not in self.sessions:
            handler._json(404 if session else 400, {"error": "unknown or missing session"})
            return
        if not headers.get("mcp-protocol-version"):
            handler._json(400, {"error": "missing MCP-Protocol-Version"})
            return
        if method is None or method.startswith("notifications/"):
            handler._empty(202)
            return
        with self.lock:
            forget = self.spec.get("forget_session_on") == method and not self.forgot
            if forget:
                self.forgot = True
                self.sessions.discard(session)
        if forget:
            handler._json(404, {"error": "session not found"})
            return
        if method == "tools/list":
            tools = self.spec.get("tools", [])
            size = self.spec.get("page_size") or max(len(tools), 1)
            start = int((body.get("params") or {}).get("cursor") or 0)
            page = {"tools": tools[start:start + size]}
            if start + size < len(tools):
                page["nextCursor"] = str(start + size)
            self._reply(handler, body["id"], result=page)
        elif method == "tools/call":
            name = body["params"]["name"]
            result = self.spec.get("results", {}).get(name)
            if result is None:
                self._reply(handler, body["id"], error={"code": -32602, "message": f"unknown tool {name}"})
            else:
                self._reply(handler, body["id"], result=result, stream=self.spec.get("sse", False))
        else:
            self._reply(handler, body["id"], error={"code": -32601, "message": f"method {method} not found"})

    def _reply(self, handler, request_id, result=None, error=None, stream=False):
        message = {"jsonrpc": "2.0", "id": request_id}
        if error:
            message["error"] = error
        else:
            message["result"] = result
        if not stream:
            handler._json(200, message)
            return
        handler.send_response(200)
        handler.send_header("content-type", "text/event-stream")
        handler.end_headers()
        progress = {"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": 1, "progress": 1}}
        ping = {"jsonrpc": "2.0", "id": "server-ping-1", "method": "ping"}
        for event in (progress, ping, message):
            handler.wfile.write(f": keepalive\n\nevent: message\ndata: {json.dumps(event)}\n\n".encode())
            handler.wfile.flush()


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


def make_handler(state, mcp=None):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            # /v1/models probes: answer with the one scripted model.
            self._json(200, {"object": "list", "data": [{"id": "e2e-model", "object": "model"}]})

        def do_POST(self):
            length = int(self.headers.get("content-length", "0"))
            body = json.loads(self.rfile.read(length) or b"{}")
            path = self.path.rstrip("/")
            if mcp and path == mcp.path:
                mcp.handle(self, body)
                return
            if not path.endswith(("/chat/completions", "/responses", "/messages")):
                self._json(404, {"error": {"message": f"unexpected path {self.path}"}})
                return
            reply = state.next_reply({**body, "_path": path})
            n = state.count
            if "status" in reply:
                # Scripted HTTP failure: {"status": 500, "error": "..."}.
                self._json(reply["status"], {"error": {"message": reply.get("error", "scripted failure")}})
                return
            if path.endswith("/responses"):
                self._responses(body, reply, n)
                return
            if path.endswith("/messages"):
                self._messages(body, reply, n)
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

        def _event(self, kind, payload):
            self.wfile.write(f"event: {kind}\ndata: {json.dumps(payload)}\n\n".encode())
            self.wfile.flush()

        def _begin_stream(self):
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.end_headers()

        def _responses(self, body, reply, n):
            output = []
            text = reply.get("text", "")
            if reply.get("reasoning"):
                output.append({"type":"reasoning", "id":f"rs-{n}", "summary":[], "encrypted_content":reply["reasoning"]})
            output += reply.get("extra_output", [])
            if text:
                output.append({"type":"message", "id":f"msg-{n}", "role":"assistant", "status":"completed", "content":[{"type":"output_text", "text":text, "annotations":[]}]})
            for i, call in enumerate(reply.get("tool_calls", [])):
                args = call["arguments"] if isinstance(call.get("arguments"), str) else json.dumps(call.get("arguments", {}))
                output.append({"type":"function_call", "id":f"fc-{n}-{i}", "call_id":f"call_{n}_{i}", "name":call["name"], "arguments":args, "status":"completed"})
            response = {"id":f"resp-{n}", "object":"response", "status":"completed", "model":body.get("model", "e2e-model"), "output":output, "usage":{"input_tokens":10, "output_tokens":5, "total_tokens":15}}
            if not body.get("stream"):
                self._json(200, response)
                return
            self._begin_stream()
            self._event("response.created", {"type":"response.created", "response":{**response, "status":"in_progress", "output":[]}})
            for i, item in enumerate(output):
                if item["type"] == "message":
                    self._event("response.output_text.delta", {"type":"response.output_text.delta", "output_index":i, "delta":text})
                elif item["type"] == "function_call":
                    args = item["arguments"]
                    for chunk in [args[:len(args)//2], args[len(args)//2:]]:
                        self._event("response.function_call_arguments.delta", {"type":"response.function_call_arguments.delta", "output_index":i, "item_id":item["id"], "delta":chunk})
                self._event("response.output_item.done", {"type":"response.output_item.done", "output_index":i, "item":item})
            # A legitimate stream may carry items only in output_item.done.
            self._event("response.completed", {"type":"response.completed", "response":{**response, "output":[]}})

        def _messages(self, body, reply, n):
            content = []
            if reply.get("text"):
                content.append({"type":"text", "text":reply["text"]})
            for i, call in enumerate(reply.get("tool_calls", [])):
                args = call.get("arguments", {})
                content.append({"type":"tool_use", "id":f"call_{n}_{i}", "name":call["name"], "input":json.loads(args) if isinstance(args, str) else args})
            stop = "tool_use" if reply.get("tool_calls") else "end_turn"
            message = {"id":f"msg-{n}", "type":"message", "role":"assistant", "model":body.get("model", "e2e-model"), "content":content, "stop_reason":stop, "stop_sequence":None, "usage":{"input_tokens":10, "output_tokens":5}}
            if not body.get("stream"):
                self._json(200, message)
                return
            self._begin_stream()
            self._event("message_start", {"type":"message_start", "message":{**message, "content":[], "stop_reason":None, "usage":{"input_tokens":10, "output_tokens":0}}})
            for i, block in enumerate(content):
                initial = {**block, "input":{}} if block["type"] == "tool_use" else {"type":"text", "text":""}
                self._event("content_block_start", {"type":"content_block_start", "index":i, "content_block":initial})
                text = json.dumps(block["input"]) if block["type"] == "tool_use" else block["text"]
                for chunk in [text[:len(text)//2], text[len(text)//2:]]:
                    delta = {"type":"input_json_delta", "partial_json":chunk} if block["type"] == "tool_use" else {"type":"text_delta", "text":chunk}
                    self._event("content_block_delta", {"type":"content_block_delta", "index":i, "delta":delta})
                self._event("content_block_stop", {"type":"content_block_stop", "index":i})
            self._event("message_delta", {"type":"message_delta", "delta":{"stop_reason":stop, "stop_sequence":None}, "usage":{"output_tokens":5}})
            self._event("message_stop", {"type":"message_stop"})

        def _sse(self, chunk, n):
            chunk = {"id": f"cmpl-{n}", "object": "chat.completion.chunk", "model": "e2e-model", **chunk}
            self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
            self.wfile.flush()

        def _json(self, code, payload, headers=None):
            data = json.dumps(payload).encode()
            self.send_response(code)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(data)))
            for name, value in (headers or {}).items():
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(data)

        def _empty(self, code):
            self.send_response(code)
            self.send_header("content-length", "0")
            self.end_headers()

    return Handler


def main():
    script_path, log_path, port_file = sys.argv[1:4]
    with open(script_path, encoding="utf-8") as f:
        script = json.load(f)
    mcp = None
    if len(sys.argv) > 5:
        with open(sys.argv[4], encoding="utf-8") as f:
            mcp = FakeMcp(json.load(f), sys.argv[5])
    server = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(State(script, log_path), mcp))
    with open(port_file, "w", encoding="utf-8") as f:
        f.write(str(server.server_address[1]))
    server.serve_forever()


if __name__ == "__main__":
    main()
