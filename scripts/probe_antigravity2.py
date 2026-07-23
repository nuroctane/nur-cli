#!/usr/bin/env python3
import http.client
import json
import ssl
from pathlib import Path

ps = json.loads(Path.home().joinpath(".nur/provider_sessions.json").read_text(encoding="utf-8"))
ag = ps["antigravity"]
token = ag["api_key"]
proj = (ag.get("oauth_meta") or {}).get("extra", {}).get("project_id", "vivid-question-5fs6l")
host = "cloudcode-pa.googleapis.com"
ctx = ssl.create_default_context()
meta = {
    "ideType": "IDE_UNSPECIFIED",
    "platform": "PLATFORM_UNSPECIFIED",
    "pluginType": "GEMINI",
}


def post(path: str, body: dict, extra_headers: dict | None = None):
    data = json.dumps(body).encode()
    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
        "User-Agent": "google-api-nodejs-client/9.15.1",
        "X-Goog-Api-Client": "google-cloud-sdk vscode_cloudshelleditor/0.1",
        "Client-Metadata": json.dumps(meta),
    }
    if extra_headers:
        headers.update(extra_headers)
    conn = http.client.HTTPSConnection(host, timeout=25, context=ctx)
    try:
        conn.request("POST", path, body=data, headers=headers)
        r = conn.getresponse()
        txt = r.read().decode("utf-8", "replace")
        return r.status, txt[:2000]
    finally:
        conn.close()


print("project", proj, "token_len", len(token))
for name, path, body, hdrs in [
    ("load", "/v1internal:loadCodeAssist", {"metadata": meta}, None),
    (
        "gen",
        "/v1internal:generateContent",
        {
            "project": proj,
            "model": "gemini-2.5-flash",
            "request": {"contents": [{"role": "user", "parts": [{"text": "hi"}]}]},
        },
        None,
    ),
    (
        "gen+x-goog-user-project",
        "/v1internal:generateContent",
        {
            "project": proj,
            "model": "gemini-2.5-flash",
            "request": {"contents": [{"role": "user", "parts": [{"text": "hi"}]}]},
        },
        {"x-goog-user-project": proj},
    ),
    (
        "onboard",
        "/v1internal:onboardUser",
        {
            "tierId": "free-tier",
            "metadata": meta,
            "cloudaicompanionProject": proj,
        },
        None,
    ),
    (
        "onboard-empty-project",
        "/v1internal:onboardUser",
        {"tierId": "free-tier", "metadata": meta},
        None,
    ),
]:
    print(f"\n=== {name} ===")
    try:
        status, text = post(path, body, hdrs)
        print(status)
        print(text)
    except Exception as e:
        print("EXC", type(e).__name__, e)
