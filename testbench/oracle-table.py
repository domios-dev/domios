#!/usr/bin/env python3
"""Unified oracle table over every committed timeline.

One row per timeline: final state, tail3s_working, question tail (trailing-window
question frames), mid-build question, card frames, total frames, and STALE/fresh
vs the production classifier source mtime.

A row's `detector` is the BAKED field from timeline.json (rendered by
render_timeline.rs). STALE means the classifier source changed after this
timeline was rendered -> regenerate before trusting the numbers
(`testbench/acceptance-suite.sh --regen`).

Usage:
  python3 testbench/oracle-table.py            # all timelines
  python3 testbench/oracle-table.py --canonical # acceptance corpus only
  python3 testbench/oracle-table.py --json
"""
import argparse
import glob
import json
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPLAYS = ROOT / "replays"
CLASSIFIER_SRC = [
    ROOT.parent / "src-tauri/src/agent/offline_state_classifier.rs",
    ROOT.parent / "src-tauri/src/agent/classify_input.rs",
    ROOT.parent / "src-tauri/src/agent/signal_extractor.rs",
    ROOT.parent / "src-tauri/examples/render_timeline.rs",
]
CANONICAL = (
    "provider_cli_question_probe_v1",
    "codex_prompt_idle_regression_v1",
    "terminal_questions_choice_v1",
    "idle_focus_churn_v1",
    "snake_incremental_build_browser",
    "systems_design_long_chat_idle_v1",
)


def classifier_mtime():
    return max((p.stat().st_mtime for p in CLASSIFIER_SRC if p.exists()), default=0)


def det(f):
    return f.get("detector") or {}


def row(path, cls_mtime):
    d = json.load(open(path))
    fr = d.get("frames", [])
    if not fr:
        return None
    end = fr[-1]["t_ms"]
    tail = [f for f in fr if f["t_ms"] >= end - 3000]
    counts = {}
    for f in fr:
        s = det(f).get("state")
        counts[s] = counts.get(s, 0) + 1
    # mid-build question = after the leading contiguous idle/question startup region
    startup_end = 0
    for f in fr:
        if det(f).get("state") in ("idle", "question"):
            startup_end = f["t_ms"]
        else:
            break
    final = next((det(f).get("state") for f in reversed(fr) if det(f).get("state")), None)
    return {
        "name": Path(path).parent.name,
        "frames": len(fr),
        "final": final,
        "tail3s_working": sum(1 for f in tail if det(f).get("state") == "working"),
        "question_tail": sum(1 for f in tail if det(f).get("state") == "question"),
        "mid_q": sum(1 for f in fr if det(f).get("state") == "question" and f["t_ms"] > startup_end),
        "cards": sum(1 for f in fr if det(f).get("card")),
        "q_total": counts.get("question", 0),
        "stale": os.path.getmtime(path) < cls_mtime,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--canonical", action="store_true")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    cls_mtime = classifier_mtime()
    paths = sorted(glob.glob(str(REPLAYS / "**/timeline.json"), recursive=True))
    if args.canonical:
        paths = [p for p in paths if any(c in p for c in CANONICAL)]
    rows = [r for r in (row(p, cls_mtime) for p in paths) if r]

    if args.json:
        print(json.dumps({"classifier_mtime": cls_mtime, "rows": rows}, indent=2, ensure_ascii=False))
        return

    nstale = sum(1 for r in rows if r["stale"])
    print(f"# {len(rows)} timelines | classifier src mtime epoch={int(cls_mtime)} | STALE={nstale} fresh={len(rows)-nstale}\n")
    h = f"{'final':>7} {'tail3sW':>7} {'qTail':>5} {'midQ':>4} {'cards':>5} {'frames':>6} {'st':>3}  name"
    print(h)
    print("-" * len(h))
    for r in sorted(rows, key=lambda r: r["name"]):
        st = "ST" if r["stale"] else "ok"
        bad = "" if (r["final"] == "idle" and r["tail3s_working"] == 0 and r["cards"] == 0 and r["mid_q"] == 0) else "  <-- review"
        print(f"{str(r['final']):>7} {r['tail3s_working']:>7} {r['question_tail']:>5} {r['mid_q']:>4} {r['cards']:>5} {r['frames']:>6} {st:>3}  {r['name'][:60]}{bad}")
    print(f"\nlegend: tail3sW=working frames in last 3s (stuck-Working oracle, want 0 for idle-ending)")
    print(f"        qTail=question frames in last 3s | midQ=question after startup | cards=inline card frames (want 0)")
    print(f"        st=ST means STALE vs classifier src -> regenerate (acceptance-suite.sh --regen)")


if __name__ == "__main__":
    main()
