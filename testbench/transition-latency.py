#!/usr/bin/env python3
"""Working->Idle transition-latency table per provider/replay/turn.

For every working->idle edge in a timeline, reports the quiescence the classifier
saw WHEN it declared idle:
  * semantic_idle_ms @ idle  = how long the (status-bar-excluded) screen had been
    unchanged when idle fired  == the detection latency we care about.
  * byte_idle_ms @ idle       = how long since any byte.
  * cause (inferred from the current classifier branches):
      - ready_prompt/composer : either clock < STUCK_WORKING_IDLE_MS (5000ms) when
        idle fired -> the composer/ready-prompt branch must have won (the safety net
        requires BOTH clocks >= 5000).
      - safety-net            : both clocks >= 5000ms -> the stuck-Working safety net
        is the only branch that idles at that quiescence.
    (Exact branch is not baked per-frame; this is the observable inference. The
    quiescence/screen fields come from signal_extractor.rs, valid regardless of the
    classifier edit; run against FRESH renders for current-classifier latency.)

Usage:
  python3 testbench/transition-latency.py <dir-of-timelines>     # e.g. scratchpad/regen
  python3 testbench/transition-latency.py --replays             # committed testbench/replays (may be STALE)
  python3 testbench/transition-latency.py <dir> --json
"""
import argparse
import glob
import json
import sys
from pathlib import Path

STUCK_MS = 5000  # STUCK_WORKING_IDLE_MS in offline_state_classifier.rs


def det(f):
    return f.get("detector") or {}


def quis(f):
    q = f.get("quiescence") or {}
    return q.get("byte_idle_ms"), q.get("semantic_idle_ms")


def provider_of(d, path):
    return d.get("provider") or next((p for p in ("codex", "opencode", "cursor", "claude") if p in path), "?")


def transitions(path):
    d = json.load(open(path))
    fr = d.get("frames", [])
    rows = []
    prev = None
    last_working_t = None
    turn = 0
    for f in fr:
        s = det(f).get("state")
        if s == "working":
            last_working_t = f["t_ms"]
        if prev == "working" and s == "idle":
            turn += 1
            b, sem = quis(f)
            cause = "?"
            if b is not None and sem is not None:
                cause = "safety-net" if (b >= STUCK_MS and sem >= STUCK_MS) else "ready_prompt/composer"
            rows.append({
                "provider": provider_of(d, path),
                "replay": Path(path).stem if "__" in Path(path).stem else Path(path).parent.name,
                "turn": turn,
                "working_end_t_ms": last_working_t,
                "idle_t_ms": f["t_ms"],
                "byte_idle_ms": b,
                "semantic_idle_ms": sem,
                "cause": cause,
            })
        prev = s
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dir", nargs="?")
    ap.add_argument("--replays", action="store_true")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--gate", action="store_true",
                    help="PASS/FAIL per provider: PASS = 0 safety-net transitions "
                         "(working->idle resolves via structural composer anchor, not the 5s timeout).")
    args = ap.parse_args()

    if args.replays:
        paths = sorted(glob.glob(str(Path(__file__).resolve().parent / "replays/**/timeline.json"), recursive=True))
    elif args.dir:
        paths = sorted(glob.glob(str(Path(args.dir) / "*.json")))
    else:
        ap.error("need a dir or --replays")

    rows = []
    for p in paths:
        try:
            rows.extend(transitions(p))
        except Exception as e:
            print(f"skip {p}: {e}", file=sys.stderr)

    if args.json:
        print(json.dumps(rows, indent=2, ensure_ascii=False))
        return

    if args.gate:
        from collections import defaultdict
        agg = defaultdict(lambda: {"rp": 0, "sn": 0, "sem": [], "sn_rows": []})
        for r in rows:
            a = agg[r["provider"]]
            a["rp" if r["cause"] == "ready_prompt/composer" else "sn"] += 1
            if r["semantic_idle_ms"] is not None:
                a["sem"].append(r["semantic_idle_ms"])
            if r["cause"] == "safety-net":
                a["sn_rows"].append(r)
        all_ok = True
        for p, a in sorted(agg.items()):
            s = sorted(a["sem"]); med = s[len(s)//2] if s else 0
            ok = a["sn"] == 0
            all_ok &= ok
            print(f"[{'PASS' if ok else 'FAIL'}] {p:>9}  transitions={a['rp']+a['sn']:>3}  "
                  f"composer-anchor={a['rp']:>3}  safety-net={a['sn']:>2}  sem_med={med}ms sem_max={(s[-1] if s else 0)}ms")
            for r in a["sn_rows"]:
                print(f"        safety-net: t={r['idle_t_ms']}ms semIdle={r['semantic_idle_ms']}  turn{r['turn']}  {r['replay'][:56]}")
        print(f"\nGATE: {'GREEN' if all_ok else 'RED'}  (PASS = working->idle via structural composer anchor, 0 safety-net fallbacks)")
        sys.exit(0 if all_ok else 1)

    h = f"{'provider':>9} {'turn':>4} {'work_end':>9} {'idle_t':>9} {'byteIdle':>9} {'semIdle':>9}  {'cause':<22} replay"
    print(h)
    print("-" * len(h))
    for r in sorted(rows, key=lambda r: (r["provider"], r["replay"], r["turn"])):
        print(f"{r['provider']:>9} {r['turn']:>4} {str(r['working_end_t_ms']):>9} {str(r['idle_t_ms']):>9} "
              f"{str(r['byte_idle_ms']):>9} {str(r['semantic_idle_ms']):>9}  {r['cause']:<22} {r['replay'][:46]}")
    # summary
    n = len(rows)
    rp = sum(1 for r in rows if r["cause"] == "ready_prompt/composer")
    sn = sum(1 for r in rows if r["cause"] == "safety-net")
    sems = [r["semantic_idle_ms"] for r in rows if r["semantic_idle_ms"] is not None]
    print(f"\n{n} transitions | ready_prompt/composer={rp} safety-net={sn}")
    if sems:
        sems.sort()
        print(f"semantic_idle_ms @ idle: min={sems[0]} med={sems[len(sems)//2]} max={sems[-1]}")
    print(f"(safety-net at semantic_idle>={STUCK_MS}ms means ready_prompt did NOT match the composer — a detection gap to close)")


if __name__ == "__main__":
    main()
