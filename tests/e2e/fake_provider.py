"""Scripted OpenAI-compatible Chat Completions server for nur's E2E suite.

Each POST /v1/chat/completions pops the next scripted reply and logs the full
request body, so a scenario can assert on what nur actually sent (tool results,
system prompt, message order) as well as on what it did.

A reply is {"text": "..."}, {"tool_calls": [{"name", "arguments"}]} (a string
"arguments" is sent verbatim, so malformed JSON can be scripted), or
{"status": 500, "error": "..."} for an HTTP failure ({"status": 404, "html": "..."}
answers with a web page instead; "body" replaces the JSON error body and
"headers" adds response headers, such as a usage limit's reset). A reply with
"drop": "before_response" resets the connection without answering, and
"drop": "mid_stream" streams its text then resets: both are transport failures,
not HTTP errors. "stream_delay": S streams the text one line every S seconds,
so a client sees output keep arriving. On the Responses wire a
reply may also carry "reasoning" (an encrypted reasoning item, as OpenAI returns
with store=false) and "extra_output" (raw output items such as Perplexity's
search_results), both emitted before the message. On Chat Completions a reply
may carry "reasoning_content", the hidden thinking DeepSeek and Kimi return.
When the script runs out the server answers with a fixed text, so a runaway
loop ends instead of hanging; the runner then fails on the request count.

With an MCP spec it also serves a Streamable HTTP MCP server (the Enclave
stand-in) on the same port; see FakeMcp.

Under /console it plays OpenCode Console's device sign-in: a code, one
"authorization_pending" poll, then tokens (console-access-N; the first one
expires in 60s so a client refreshes it), refresh_token grants, plus /api/orgs
and /api/user for a bearer that holds one of those tokens.

Usage: python fake_provider.py <script.json> <requests.jsonl> <port-file> [<mcp.json> <mcp.jsonl>]
"""

import json
import os
import socket
import struct
import sys
import threading
import time
import socketserver
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# Where `serve_files` land (next to the script); set in main().
RAW_DIR = Path(".")

EXHAUSTED = "[fake provider: script exhausted]"

CONSOLE = {"polls": 0, "issued": 0}
CONSOLE_LOCK = threading.Lock()


class LoopbackHTTPServer(ThreadingHTTPServer):
    """ThreadingHTTPServer without the reverse-DNS lookup in server_bind.

    HTTPServer.server_bind sets server_name from socket.getfqdn(host), which on
    macOS (GitHub's runners included) can stall for tens of seconds before the
    socket is usable. Nothing here reads server_name, so skip the lookup.
    """

    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        host, port = self.server_address[:2]
        self.server_name = host
        self.server_port = port


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

        def _console(self, path, body):
            route = path[len("/console"):]
            with CONSOLE_LOCK:
                if route == "/auth/device/code":
                    if body.get("client_id") != "opencode-cli":
                        return self._json(400, {"error": "invalid_client"})
                    return self._json(200, {"device_code": "dev-1", "user_code": "ABCD-1234",
                                            "verification_uri_complete": "device?code=ABCD-1234",
                                            "expires_in": 120, "interval": 1})
                if route == "/auth/device/token":
                    grant = body.get("grant_type", "")
                    if grant.endswith("device_code"):
                        CONSOLE["polls"] += 1
                        if CONSOLE["polls"] == 1:
                            return self._json(400, {"error": "authorization_pending"})
                    elif grant != "refresh_token" or not str(body.get("refresh_token", "")).startswith("console-refresh-"):
                        return self._json(400, {"error": "invalid_grant"})
                    CONSOLE["issued"] += 1
                    n = CONSOLE["issued"]
                    return self._json(200, {"access_token": f"console-access-{n}", "refresh_token": f"console-refresh-{n}",
                                            # The sign-in's token is near expiry, so the
                                            # first turn must refresh it at the Console.
                                            "expires_in": 60 if n == 1 else 3600, "org_id": None})
                token = self.headers.get("authorization", "")
                if not token.startswith("Bearer console-access-"):
                    return self._json(401, {"_tag": "Unauthorized"})
                if route == "/api/orgs":
                    return self._json(200, [{"id": "org-b", "name": "Zeta"}, {"id": "org-a", "name": "Acme"}])
                if route == "/api/user":
                    return self._json(200, {"id": "user-1", "email": "e2e@example.test"})
            self._json(404, {"error": f"unexpected console path {path}"})

        def do_GET(self):
            path = self.path.rstrip("/")
            if path.startswith("/console/"):
                return self._console(path, {})
            if path.startswith("/raw/"):
                # Files a scenario serves (`serve_files`), e.g. a skill at a
                # pinned commit standing in for raw.githubusercontent.com.
                return self._raw(path[len("/raw/"):])
            # /v1/models probes: answer with the one scripted model.
            self._json(200, {"object": "list", "data": [{"id": "e2e-model", "object": "model"}]})

        def _raw(self, rel):
            target = (RAW_DIR / rel).resolve()
            if RAW_DIR.resolve() not in target.parents or not target.is_file():
                self._json(404, {"error": {"message": f"no served file {rel}"}})
                return
            data = target.read_bytes()
            self.send_response(200)
            self.send_header("content-type", "application/octet-stream")
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_POST(self):
            length = int(self.headers.get("content-length", "0"))
            body = json.loads(self.rfile.read(length) or b"{}")
            path = self.path.rstrip("/")
            if path.startswith("/console/"):
                return self._console(path, body)
            if mcp and path == mcp.path:
                mcp.handle(self, body)
                return
            if path.startswith("/call/"):
                self._treg(path[len("/call/"):], body)
                return
            if not path.endswith(("/chat/completions", "/responses", "/messages")):
                self._json(404, {"error": {"message": f"unexpected path {self.path}"}})
                return
            # Headers are recorded so a scenario can assert on them: cache
            # affinity and client identity are wire facts a body check misses.
            headers = {name.lower(): value for name, value in self.headers.items()}
            reply = state.next_reply({**body, "_path": path, "_headers": headers})
            n = state.count
            drop = reply.get("drop")
            if drop == "before_response":
                self._reset()
                return
            if "status" in reply and "html" in reply:
                # A website answering instead of an API (a wrong base URL).
                page = reply["html"].encode()
                self.send_response(reply["status"])
                self.send_header("content-type", "text/html; charset=utf-8")
                self.send_header("content-length", str(len(page)))
                self.end_headers()
                self.wfile.write(page)
                return
            if "status" in reply:
                # Scripted HTTP failure: {"status": 500, "error": "..."}.
                payload = reply.get("body") or {"error": {"message": reply.get("error", "scripted failure")}}
                self._json(reply["status"], payload, reply.get("headers"))
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
                if reply.get("reasoning_content"):
                    self._sse({"choices": [{"index": 0, "delta": {**delta, "reasoning_content": reply["reasoning_content"]}, "finish_reason": None}]}, n)
                pace = reply.get("stream_delay")
                if text and pace:
                    for line in text.splitlines(keepends=True):
                        self._sse({"choices": [{"index": 0, "delta": {**delta, "content": line}, "finish_reason": None}]}, n)
                        time.sleep(float(pace))
                    text = ""
                if text:
                    delta["content"] = text
                if calls:
                    delta["tool_calls"] = calls
                self._sse({"choices": [{"index": 0, "delta": delta, "finish_reason": None}]}, n)
                if drop == "mid_stream":
                    time.sleep(0.2)  # let the client read the partial text first
                    self._reset()
                    return
                self._sse({"choices": [{"index": 0, "delta": {}, "finish_reason": finish}], "usage": usage}, n)
                self.wfile.write(b"data: [DONE]\n\n")
                self.wfile.flush()
                return
            message = {"role": "assistant", "content": text or None}
            if reply.get("reasoning_content"):
                message["reasoning_content"] = reply["reasoning_content"]
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

        def _treg(self, tool, body):
            """A stand-in for treg.to's routed web jobs. It refuses calls without
            the token or the per-call cost ceiling, so a passing scenario proves
            nur sends both."""
            if not self.headers.get("x-treg-token"):
                self._json(401, {"message": "missing X-Treg-Token"})
                return
            if not self.headers.get("x-treg-route-max-cost"):
                self._json(400, {"message": "e2e: X-Treg-Route-Max-Cost is required"})
                return
            served = {"served_by": self.headers.get("x-treg-route-prefer", "?"), "charged_micro": 0}
            if tool == "treg.web.search":
                self._json(200, {"output": {"results": [{
                    "title": "treg web access",
                    "url": "https://treg.to/web-access",
                    "snippet": f"free search for {body.get('q', '')}",
                }]}, "_treg": served})
            elif tool == "treg.web.extract":
                self._json(200, {"output": {"markdown": f"# Read by treg\n{body.get('url', '')}"}, "_treg": served})
            else:
                self._json(404, {"message": f"no tool {tool}"})

        def _reset(self):
            """Close with RST, not FIN, so the client sees a dropped connection
            rather than a body that ended cleanly."""
            linger = struct.pack("HH" if os.name == "nt" else "ii", 1, 0)
            self.connection.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, linger)
            self.close_connection = True
            self.connection.close()

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
    global RAW_DIR
    script_path, log_path, port_file = sys.argv[1:4]
    RAW_DIR = Path(script_path).parent / "raw"
    with open(script_path, encoding="utf-8") as f:
        script = json.load(f)
    mcp = None
    if len(sys.argv) > 5:
        with open(sys.argv[4], encoding="utf-8") as f:
            mcp = FakeMcp(json.load(f), sys.argv[5])
    server = LoopbackHTTPServer(("127.0.0.1", 0), make_handler(State(script, log_path), mcp))
    with open(port_file, "w", encoding="utf-8") as f:
        f.write(str(server.server_address[1]))
    server.serve_forever()


if __name__ == "__main__":
    main()
