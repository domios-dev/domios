#!/usr/bin/env python3
"""Record WHEN canonical chat messages become readable for a tab.

Dev-only sidecar for streaming-preview quality runs: samples
GET /api/tabs/{id}/chat?since=N on the dev daemon (port 7686) and appends one
NDJSON row per newly visible message batch, stamped with wall-clock ms. The
row timeline is later aligned to the .cast clock via the capture's
captured_at_ms and replayed by simulate.ts.

Usage:
  python testbench/streaming/record-chat-timeline.py --tab TAB_ID \
      --out chat-timeline.ndjson [--duration-s 600] [--interval-ms 150]
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "evals" / "_lib"))
from dev_connection import dev_api_url, dev_environment, dev_token


def request(path: str, token: str, timeout_s: float, api_url: str):
    req = urllib.request.Request(
        f"{api_url}{path}",
        headers={"Authorization": f"Bearer {token}"} if token else {},
    )
    return urllib.request.urlopen(req, timeout=timeout_s)


def read_chat(tab_id: str, since: int, token: str, api_url: str) -> dict:
    with request(f"/api/tabs/{tab_id}/chat?since={since}", token, 5, api_url) as resp:
        return json.load(resp)


def write_messages(sink, seen: int, messages: list[dict]) -> int:
    if not messages:
        return seen
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
    return seen + len(messages)


# Polling, not SSE: `tab-chat-{id}` deltas are only emitted while a Chat UI
# sink is attached, so the general event stream is silent for headless probes.
def poll_chat(tab_id: str, token: str, sink, duration_s: int, interval_ms: int, api_url: str) -> None:
    seen = 0
    deadline = time.monotonic() + duration_s
    while time.monotonic() < deadline:
        try:
            doc = read_chat(tab_id, seen, token, api_url)
        except Exception as error:  # noqa: BLE001 — sampler keeps going
            print(f"chat read failed: {error}", file=sys.stderr)
            time.sleep(interval_ms / 1000)
            continue
        messages = doc.get("messages", [])
        if messages:
            write_messages(sink, seen, messages)
            seen = doc.get("count", seen + len(messages))
        time.sleep(interval_ms / 1000)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tab", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--duration-s", type=int, default=600)
    parser.add_argument("--interval-ms", type=int, default=150)
    args = parser.parse_args()

    connection = dev_environment()
    token = dev_token(connection)
    api_url = dev_api_url(connection)
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8") as sink:
        sink.write(json.dumps({"kind": "start", "wall_ms": int(time.time() * 1000), "tab": args.tab}) + "\n")
        sink.flush()
        poll_chat(args.tab, token, sink, args.duration_s, args.interval_ms, api_url)
    return 0


if __name__ == "__main__":
    sys.exit(main())
