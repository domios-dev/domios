#!/usr/bin/env python3
"""Per-transition idle-latency oracle. At every working->idle transition, report
the byte_idle_ms / semantic_idle_ms recorded AT the transition frame. A transition
that only fires once byte_idle has reached the stuck-Working safety-net threshold
(~5000ms) settled via TIMEOUT, not a structural ready signal — that is a FAIL
(visible false-Working window + indefinite-stuck risk under repaint). A structural
settle fires at byte_idle <= WORKING_GRACE_MS (1500) + a small sampling slop.

Usage: idle_latency_oracle.py <timeline.json> [--grace-ms 1500] [--net-ms 5000]
Exit 0 = all transitions structural, 1 = at least one timeout settle.
"""
import json, sys, argparse

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("timeline")
    ap.add_argument("--grace-ms", type=int, default=1500)
    ap.add_argument("--net-ms", type=int, default=5000)
    ap.add_argument("--slop-ms", type=int, default=400)
    a = ap.parse_args()
    fr = json.load(open(a.timeline))["frames"]
    prev = None
    transitions = []
    for f in fr:
        st = (f.get("detector") or {}).get("state")
        if prev == "working" and st == "idle":
            q = f.get("quiescence") or {}
            transitions.append((f.get("t_ms"), q.get("byte_idle_ms"), q.get("semantic_idle_ms")))
        prev = st
    if not transitions:
        print("  (no working->idle transitions)"); return 0
    bad = 0
    for t, b, s in transitions:
        # timeout settle = byte_idle at/above (net - slop): only the 5s net could have fired it
        timeout = b is not None and b >= a.net_ms - a.slop_ms
        structural = b is not None and b <= a.grace_ms + a.slop_ms
        tag = "TIMEOUT-SETTLE(FAIL)" if timeout else ("structural" if structural else "grace-window")
        if timeout:
            bad += 1
        print(f"  t={t} working->idle  byte_idle={b}ms semantic_idle={s}ms -> {tag}")
    print(f"  transitions={len(transitions)} timeout_settles={bad}")
    return 1 if bad else 0

if __name__ == "__main__":
    sys.exit(main())
