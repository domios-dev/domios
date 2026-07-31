#!/usr/bin/env python3
"""Record WHEN canonical chat messages become readable for a tab.

Dev-only sidecar for streaming-preview quality runs: samples
GET /api/tabs/{id}/chat?since=N on the dev daemon (port 7686) and appends one
NDJSON row per newly visible message batch, stamped with wall-clock ms. The
row timeline is later aligned to the .cast clock via the capture's
captured_at_ms and replayed by simulate.ts.

Usage:
  python testbench/streaming/record-chat-timeline.py --tab TAB_ID \
      --out chat-timeline.ndjson [--interval-ms 150] [--duration-s 600]
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.request
from pathlib import Path

DEV_PORT = "7686"


def dev_token() -> str:
    cfg = Path.home() / ".codeterm-dev" / "config.toml"
    if cfg.exists():
        for line in cfg.read_text(encoding="utf-8").splitlines():
            if line.strip().startswith("token = "):
                return line.split('"', 2)[1]
    return ""


def read_chat(tab_id: str, since: int, token: str) -> dict:
    req = urllib.request.Request(
        f"http://127.0.0.1:{DEV_PORT}/api/tabs/{tab_id}/chat?since={since}",
        headers={"Authorization": f"Bearer {token}"} if token else {},
    )
    with urllib.request.urlopen(req, timeout=5) as resp:
        return json.load(resp)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tab", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--interval-ms", type=int, default=150)
    parser.add_argument("--duration-s", type=int, default=600)
    args = parser.parse_args()

    token = dev_token()
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    seen = 0
    deadline = time.time() + args.duration_s
    with out.open("w", encoding="utf-8") as sink:
        sink.write(json.dumps({"kind": "start", "wall_ms": int(time.time() * 1000), "tab": args.tab}) + "\n")
        sink.flush()
        while time.time() < deadline:
            try:
                doc = read_chat(args.tab, seen, token)
            except Exception as error:  # noqa: BLE001 — sampler keeps going
                print(f"chat read failed: {error}", file=sys.stderr)
                time.sleep(args.interval_ms / 1000)
                continue
            messages = doc.get("messages", [])
            if messages:
                sink.write(
                    json.dumps(
                        {
                            "kind": "messages",
                            "wall_ms": int(time.time() * 1000),
                            "since": seen,
                            "messages": [
                                {"kind": m.get("kind") or m.get("type"), "content": m.get("content", "")}
                                for m in messages
                            ],
                        },
                        ensure_ascii=False,
                    )
                    + "\n"
                )
                sink.flush()
                seen = doc.get("count", seen + len(messages))
            time.sleep(args.interval_ms / 1000)
    return 0


if __name__ == "__main__":
    sys.exit(main())
