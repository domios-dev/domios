#!/usr/bin/env python3
"""Idle-hold gate: a negative-fixture acceptance check that compares the detector
to the EXPECTED phase lane (anti-circular). For each `idle_hold` event in
phases.json, the hold BODY (hold_start + settle_ms .. hold_end) must contain ZERO
working and ZERO question frames — a blinking cursor / focus churn is not work.
A small settle tail right after the turn (<= settle_ms) is tolerated, since the
provider's work widget can linger briefly past the scenario's byte-quiet marker.

Usage: idle_hold_gate.py <timeline.json> <phases.json> [--settle-ms 3000]
Exit 0 = clean, 1 = violation.
"""
import json, sys, argparse

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("timeline")
    ap.add_argument("phases")
    ap.add_argument("--settle-ms", type=int, default=3000)
    a = ap.parse_args()
    fr = json.load(open(a.timeline))["frames"]
    ev = json.load(open(a.phases)).get("events", [])
    holds = [(e["t_ms"], e["t_ms"] + e.get("duration_ms", 20000))
             for e in ev if e.get("kind") == "idle_hold"]
    if not holds:
        print(f"  NO idle_hold events in {a.phases}"); return 0
    bad = 0
    for start, end in holds:
        body = [f for f in fr if start + a.settle_ms <= f["t_ms"] <= end]
        w = [f["t_ms"] for f in body if (f.get("detector") or {}).get("state") == "working"]
        q = [f["t_ms"] for f in body if (f.get("detector") or {}).get("state") == "question"]
        tail = [f for f in fr if start <= f["t_ms"] < start + a.settle_ms
                and (f.get("detector") or {}).get("state") == "working"]
        status = "OK" if not w and not q else "VIOLATION"
        if w or q:
            bad += 1
        print(f"  hold {start}..{end} body: working={len(w)} question={len(q)} "
              f"(settle-tail working={len(tail)}) -> {status}")
    return 1 if bad else 0

if __name__ == "__main__":
    sys.exit(main())
