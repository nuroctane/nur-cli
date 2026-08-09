#!/usr/bin/env python3
"""Nur helper: compress one tool-result blob via Headroom's library API.

Reads UTF-8 text from stdin (or --file), prints compressed text to stdout.
Exit 0 on success. Exit 2 if headroom is not importable. Exit 1 on other errors.

Tool bodies are sent as role=tool so Headroom's coding defaults actually crush
them (user-role messages are protected / skipped by default).
"""

from __future__ import annotations

import argparse
import json
import sys


def main() -> int:
    parser = argparse.ArgumentParser(add_help=True)
    parser.add_argument("--model", default="gpt-4o")
    parser.add_argument("--label", default="tool")
    parser.add_argument(
        "--file",
        default="",
        help="Read text from this path instead of stdin",
    )
    parser.add_argument(
        "--json-out",
        action="store_true",
        help="Emit JSON with content + tokens_saved + ratio",
    )
    args = parser.parse_args()

    try:
        import headroom  # type: ignore
        from headroom import compress  # type: ignore
    except Exception as e:  # noqa: BLE001
        sys.stderr.write(f"headroom import failed: {e}\n")
        return 2

    if args.file:
        try:
            with open(args.file, "r", encoding="utf-8", errors="replace") as f:
                text = f.read()
        except OSError as e:
            sys.stderr.write(f"read failed: {e}\n")
            return 1
    else:
        text = sys.stdin.read()
    if not text:
        return 0

    # role=tool is required: Headroom coding defaults skip compressing user msgs.
    messages = [
        {
            "role": "tool",
            "tool_call_id": "nur_headroom",
            "content": f"[{args.label}]\n{text}",
        }
    ]
    try:
        result = compress(messages, model=args.model)
    except Exception as e:  # noqa: BLE001
        sys.stderr.write(f"compress failed: {e}\n")
        return 1

    out_msgs = getattr(result, "messages", None) or messages
    content = ""
    if out_msgs:
        raw = out_msgs[0].get("content", "")
        if isinstance(raw, list):
            parts = []
            for block in raw:
                if isinstance(block, dict) and block.get("type") == "text":
                    parts.append(str(block.get("text", "")))
                elif isinstance(block, str):
                    parts.append(block)
            content = "\n".join(parts)
        else:
            content = str(raw)

    prefix = f"[{args.label}]\n"
    if content.startswith(prefix):
        content = content[len(prefix) :]

    if args.json_out:
        usage_raw = getattr(result, "usage", None)
        if isinstance(usage_raw, dict):
            get_usage = usage_raw.get
        else:
            get_usage = lambda key, default=None: getattr(usage_raw, key, default)

        def first_usage(*names):
            for name in names:
                value = get_usage(name)
                if value is not None:
                    return value
            return None

        processing = getattr(result, "processing", None)
        if processing not in {"local", "remote", "unknown"}:
            processing = "unknown"
        backend = (
            getattr(result, "backend", None)
            or getattr(result, "provider", None)
            or f"headroom-python@{getattr(headroom, '__version__', 'unknown')}"
        )
        payload = {
            "content": content,
            "tokens_saved": getattr(result, "tokens_saved", None),
            "compression_ratio": getattr(result, "compression_ratio", None),
            "backend": str(backend),
            "processing": processing,
            "mode": "inline",
            "model": args.model,
            "usage": {
                "input_tokens": first_usage("input_tokens", "prompt_tokens"),
                "output_tokens": first_usage("output_tokens", "completion_tokens"),
                "total_tokens": first_usage("total_tokens"),
                "cost_usd": first_usage("cost_usd", "cost"),
            },
        }
        sys.stdout.write(json.dumps(payload, ensure_ascii=False))
    else:
        sys.stdout.write(content)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
