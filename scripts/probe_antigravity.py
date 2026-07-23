#!/usr/bin/env python3
"""Probe Cloud Code endpoints with the stored antigravity OAuth session (no secrets printed)."""
from __future__ import annotations

import json
import ssl
import urllib.error
import urllib.request
from pathlib import Path

ps = json.loads(Path.home().joinpath(".nur/provider_sessions.json").read_text(encoding="utf-8"))
ag = ps["antigravity"]
token = ag["api_key"]
extra = (ag.get("oauth_meta") or {}).get("extra") or {}
proj = extra.get("project_id") or "vivid-question-5fs6l"
print("project", proj)
print("tier", extra.get("tier_id"))
print("via", extra.get("via"))
print("token_len", len(token), "prefix", token[:8] + "…")
print("expires_at", ag.get("expires_at"))

CTX = ssl.create_default_context()
META = json.dumps(
    {
        "ideType": "IDE_UNSPECIFIED",
        "platform": "PLATFORM_UNSPECIFIED",
        "pluginType": "GEMINI",
    }
)


def post(url: str, body: dict, extra_headers: dict | None = None) -> tuple[object, str]:
    data = json.dumps(body).encode()
    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
        "User-Agent": "google-api-nodejs-client/9.15.1",
        "X-Goog-Api-Client": "google-cloud-sdk vscode_cloudshelleditor/0.1",
        "Client-Metadata": META,
    }
    if extra_headers:
        headers.update(extra_headers)
    req = urllib.request.Request(url, data=data, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=20, context=CTX) as r:
            return r.status, r.read().decode("utf-8", "replace")[:2000]
    except urllib.error.HTTPError as e:
        body_txt = e.read().decode("utf-8", "replace")[:2000]
        return e.code, body_txt
    except Exception as e:  # noqa: BLE001
        return type(e).__name__, str(e)[:500]


cases = [
    (
        "loadCodeAssist",
        "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
        {"metadata": json.loads(META)},
        None,
    ),
    (
        "generateContent bare model",
        "https://cloudcode-pa.googleapis.com/v1internal:generateContent",
        {
            "project": proj,
            "model": "gemini-2.5-flash",
            "request": {
                "contents": [{"role": "user", "parts": [{"text": "say hi in one word"}]}]
            },
        },
        None,
    ),
    (
        "generateContent models/ prefix",
        "https://cloudcode-pa.googleapis.com/v1internal:generateContent",
        {
            "project": proj,
            "model": "models/gemini-2.5-flash",
            "request": {
                "contents": [{"role": "user", "parts": [{"text": "say hi in one word"}]}]
            },
        },
        None,
    ),
    (
        "generateContent + x-goog-user-project",
        "https://cloudcode-pa.googleapis.com/v1internal:generateContent",
        {
            "project": proj,
            "model": "gemini-2.5-flash",
            "request": {
                "contents": [{"role": "user", "parts": [{"text": "say hi in one word"}]}]
            },
        },
        {"x-goog-user-project": proj},
    ),
    (
        "onboardUser free-tier",
        "https://cloudcode-pa.googleapis.com/v1internal:onboardUser",
        {
            "tierId": "free-tier",
            "metadata": json.loads(META),
            "cloudaicompanionProject": proj,
        },
        None,
    ),
]

for name, url, body, hdrs in cases:
    print(f"\n=== {name} ===")
    status, text = post(url, body, hdrs)
    print("status", status)
    print(text[:1200])
