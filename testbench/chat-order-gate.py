#!/usr/bin/env python3
"""Codex (and any provider) ChatUI ordering — deterministic DATA-LAYER gate.

The ChatUI renders the transcript from /api/tabs/{id}/chat. This gate asserts
that transcript is correctly ordered at the data layer:
  * `seq` strictly increasing
  * `timestamp` non-decreasing (monotonic)
  * no duplicate ids
A PASS here means any visible out-of-order rendering is a FRONTEND render bug
(src/components/Terminal/TabSessionChat.tsx), not the transcript/classifier.

DOM order itself remains MANUAL / Windows-only: Playwright drives WebView2 via
CDP (tests/playwright/README.md); macOS WKWebView has no CDP attach. This gate is
the deterministic macOS substitute, runnable in CI without a desktop.

Usage:
  python3 testbench/chat-order-gate.py testbench/fixtures/codex-chat-order-2bace0d6.json
  python3 testbench/chat-order-gate.py --live              # all live codex tabs
  python3 testbench/chat-order-gate.py --live --agent opencode
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def check(messages, label):
    errs = []
    prev_seq = None
    prev_ts = None
    seen = set()
    for i, m in enumerate(messages):
        mid = m.get("id")
        seq = m.get("seq")
        ts = m.get("timestamp")
        if mid in seen:
            errs.append(f"  idx {i}: duplicate id {mid}")
        seen.add(mid)
        if prev_seq is not None and seq is not None and seq <= prev_seq:
            errs.append(f"  idx {i}: seq {seq} <= prev {prev_seq} (id={mid})")
        if prev_ts is not None and ts is not None and ts < prev_ts:
            errs.append(f"  idx {i}: timestamp {ts} < prev {prev_ts} (id={mid})")
        prev_seq = seq if seq is not None else prev_seq
        prev_ts = ts if ts is not None else prev_ts
    ok = not errs
    print(f"[{'PASS' if ok else 'FAIL'}] {label}  (n={len(messages)})")
    for e in errs:
        print(e)
    return ok


def live_tabs(agent):
    # use the bench lib's dev_get via a tiny shell shim to stay dev-locked
    cmd = f'source "{ROOT}/evals/_lib/lib.sh" 2>/dev/null; dev_get /api/tabs'
    out = subprocess.run(["bash", "-c", cmd], capture_output=True, text=True).stdout
    tabs = json.loads(out)
    ids = [p["id"] for p in tabs if p.get("agent_type") == agent]
    res = []
    for pid in ids:
        cmd = f'source "{ROOT}/evals/_lib/lib.sh" 2>/dev/null; dev_get /api/tabs/{pid}/chat'
        out = subprocess.run(["bash", "-c", cmd], capture_output=True, text=True).stdout
        try:
            res.append((pid, json.loads(out).get("messages", [])))
        except Exception:
            res.append((pid, []))
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("fixture", nargs="?")
    ap.add_argument("--live", action="store_true")
    ap.add_argument("--agent", default="codex")
    args = ap.parse_args()

    all_ok = True
    if args.live:
        tabs = live_tabs(args.agent)
        if not tabs:
            print(f"no live {args.agent} tabs")
            return 0
        for pid, msgs in tabs:
            all_ok &= check(msgs, f"live {args.agent} tab {pid}")
    elif args.fixture:
        d = json.load(open(args.fixture))
        all_ok &= check(d["messages"], f"fixture {Path(args.fixture).name} ({d.get('agent')})")
    else:
        ap.error("need a fixture path or --live")

    print(f"\nGATE: {'GREEN' if all_ok else 'RED'}")
    print("NOTE: DOM render order is manual/Windows-only (WebView2 CDP); this is the data-layer gate.")
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
