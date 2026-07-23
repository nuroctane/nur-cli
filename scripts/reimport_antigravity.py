#!/usr/bin/env python3
"""Re-import antigravity token from Windows Credential Manager and run free-tier setup."""
from __future__ import annotations

import http.client
import json
import ssl
import subprocess
import sys
import time
from pathlib import Path

PS = r"""
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class CredManager2 {
    [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    public static extern bool CredRead(string target, int type, int flags, out IntPtr credential);
    [DllImport("advapi32.dll")]
    public static extern void CredFree(IntPtr cred);
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    public struct CREDENTIAL {
        public int Flags; public int Type; public string TargetName; public string Comment;
        public long LastWritten; public int CredentialBlobSize; public IntPtr CredentialBlob;
        public int Persist; public int AttributeCount; public IntPtr Attributes;
        public string TargetAlias; public string UserName;
    }
    public static byte[] ReadBytes(string target) {
        IntPtr credPtr;
        if (!CredRead(target, 1, 0, out credPtr)) return null;
        var cred = (CREDENTIAL)System.Runtime.InteropServices.Marshal.PtrToStructure(credPtr, typeof(CREDENTIAL));
        byte[] bytes = new byte[cred.CredentialBlobSize];
        System.Runtime.InteropServices.Marshal.Copy(cred.CredentialBlob, bytes, 0, cred.CredentialBlobSize);
        CredFree(credPtr);
        return bytes;
    }
}
"@ -Language CSharp
$targets = @('LegacyGeneric:target=gemini:antigravity','gemini:antigravity','LegacyGeneric:target=gemini-cli:oauth','gemini-cli:oauth')
foreach ($t in $targets) {
  $b = [CredManager2]::ReadBytes($t)
  if ($b) { [Text.Encoding]::UTF8.GetString($b); break }
}
"""


def wincred_json() -> dict:
    out = subprocess.check_output(
        [
            "powershell",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            PS,
        ],
        text=True,
        errors="replace",
    ).strip()
    if not out:
        raise SystemExit("no wincred blob found for gemini:antigravity")
    return json.loads(out)


def post(token: str, path: str, body: dict) -> tuple[int, dict | str]:
    data = json.dumps(body).encode()
    conn = http.client.HTTPSConnection(
        "cloudcode-pa.googleapis.com", timeout=30, context=ssl.create_default_context()
    )
    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json",
        "User-Agent": "google-api-nodejs-client/9.15.1",
        "X-Goog-Api-Client": "google-cloud-sdk vscode_cloudshelleditor/0.1",
        "Client-Metadata": json.dumps(
            {
                "ideType": "IDE_UNSPECIFIED",
                "platform": "PLATFORM_UNSPECIFIED",
                "pluginType": "GEMINI",
            }
        ),
    }
    try:
        conn.request("POST", path, body=data, headers=headers)
        r = conn.getresponse()
        raw = r.read().decode("utf-8", "replace")
        try:
            return r.status, json.loads(raw)
        except json.JSONDecodeError:
            return r.status, raw
    finally:
        conn.close()


def project_id_of(v) -> str:
    if isinstance(v, str):
        return v
    if isinstance(v, dict):
        return str(v.get("id") or "")
    return ""


def main() -> None:
    blob = wincred_json()
    tok = blob.get("token") or blob
    access = (tok.get("access_token") or "").strip()
    refresh = tok.get("refresh_token")
    expiry = tok.get("expiry")
    if not access:
        raise SystemExit("wincred has no access_token — re-login antigravity CLI first")
    print("wincred token_len", len(access), "expiry", expiry)

    meta = {
        "ideType": "IDE_UNSPECIFIED",
        "platform": "PLATFORM_UNSPECIFIED",
        "pluginType": "GEMINI",
    }
    st, load = post(access, "/v1internal:loadCodeAssist", {"metadata": meta})
    print("loadCodeAssist", st)
    if st != 200:
        print(json.dumps(load, indent=2)[:1200])
        raise SystemExit("loadCodeAssist failed — token may be expired; re-login Antigravity CLI")

    project = project_id_of(load.get("cloudaicompanionProject"))
    current = load.get("currentTier") or {}
    allowed = load.get("allowedTiers") or []
    tier = current.get("id") or "free-tier"
    for t in allowed:
        if t.get("isDefault"):
            tier = t.get("id") or tier
            break
    print("load project", project or "(none)", "tier", tier, "currentTier", bool(current))

    # Free-tier: onboard WITHOUT project (gemini-cli rule)
    if not current or not project:
        body = {"tierId": tier or "free-tier", "metadata": meta}
        # free-tier must not include cloudaicompanionProject
        if tier and tier != "free-tier" and project:
            body["cloudaicompanionProject"] = project
            body["metadata"] = {**meta, "duetProject": project}
        for i in range(12):
            st, onboard = post(access, "/v1internal:onboardUser", body)
            print(f"onboardUser attempt {i + 1}", st, "done", onboard.get("done") if isinstance(onboard, dict) else None)
            if st != 200:
                print(str(onboard)[:800])
                time.sleep(2)
                continue
            if isinstance(onboard, dict) and onboard.get("done"):
                assigned = project_id_of(
                    (onboard.get("response") or {}).get("cloudaicompanionProject")
                )
                if assigned:
                    project = assigned
                break
            time.sleep(5)
        print("after onboard project", project or "(none)")

    if not project:
        raise SystemExit("no project id after setup")

    # Smoke generateContent
    st, gen = post(
        access,
        "/v1internal:generateContent",
        {
            "project": project,
            "model": "gemini-2.5-flash",
            "request": {
                "contents": [{"role": "user", "parts": [{"text": "Reply with the single word: pong"}]}]
            },
        },
    )
    print("generateContent", st)
    if st != 200:
        print(json.dumps(gen, indent=2)[:1500] if not isinstance(gen, str) else gen[:1500])
        raise SystemExit("generateContent still failing")
    text = ""
    try:
        parts = gen["response"]["candidates"][0]["content"]["parts"]
        text = "".join(p.get("text", "") for p in parts)
    except Exception:
        text = str(gen)[:200]
    print("model said:", text[:200])

    # Persist into nur provider_sessions
    path = Path.home() / ".nur" / "provider_sessions.json"
    data = json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}
    expires_at = None
    if expiry:
        try:
            from datetime import datetime

            expires_at = int(datetime.fromisoformat(expiry.replace("Z", "+00:00")).timestamp())
        except Exception:
            expires_at = None
    for key in ("antigravity", "google"):
        prev = data.get(key) or {}
        data[key] = {
            "api_key": access,
            "source": "oauth",
            "auth_method": "oauth",
            "provider": key,
            "refresh_token": refresh or prev.get("refresh_token"),
            "expires_at": expires_at or prev.get("expires_at"),
            "oauth_meta": {
                "issuer": "https://accounts.google.com",
                "client_id": "",
                "extra": {
                    "auth_method": "consumer",
                    "project_id": project,
                    "tier_id": tier or "free-tier",
                    "via": "reimport_antigravity.py",
                },
            },
        }
    path.write_text(json.dumps(data, indent=2), encoding="utf-8")
    # Keep auth.json in sync if active provider is antigravity/google
    auth_path = Path.home() / ".nur" / "auth.json"
    if auth_path.exists():
        auth = json.loads(auth_path.read_text(encoding="utf-8"))
        if auth.get("provider") in ("antigravity", "google") or True:
            # only update if current is google family
            if auth.get("provider") in ("antigravity", "google", None, ""):
                auth.update(data.get("antigravity", {}))
                auth_path.write_text(json.dumps(auth, indent=2), encoding="utf-8")
                print("updated auth.json")
    print("saved project", project, "to", path)
    print("OK")


if __name__ == "__main__":
    main()
