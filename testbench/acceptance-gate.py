#!/usr/bin/env python3
"""ChatUI acceptance gate over the replay corpus.

Operationalizes the runbook acceptance checks
(docs/plans/detection-rebuild-deepening-loop.md, "Final ChatUI acceptance checks")
as measurable PASS/FAIL over committed `timeline.json` files. The `detector`
field in each timeline is produced by `src-tauri/examples/render_timeline.rs`,
which runs the SAME `offline_state_classifier` the live path uses — so a green
gate here is the offline proof for a classifier change before the live re-check.

Checks that need an interactive surface (Stop/Cancel/Esc, Codex DOM ordering)
are reported as LIVE-ONLY and are NOT asserted here — they require a running
daemon + ChatUI DOM oracle, tracked in the runbook loop.

Usage:
  python3 testbench/acceptance-gate.py            # canonical corpus
  python3 testbench/acceptance-gate.py --json      # machine-readable
"""
import argparse
import glob
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPLAYS = ROOT / "replays"
STUCK_WORKING_IDLE_MS = 5000


def load(path):
    d = json.load(open(path))
    return d, d.get("frames", [])


def state_counts(frames):
    c = {}
    for f in frames:
        s = (f.get("detector") or {}).get("state")
        c[s] = c.get(s, 0) + 1
    return c


def field_frames(frames, key):
    return sum(1 for f in frames if (f.get("detector") or {}).get(key))


def first_input_ms(d):
    inp = d.get("inputs") or []
    return inp[0]["t_ms"] if inp else 0


def final_state(frames):
    for f in reversed(frames):
        s = (f.get("detector") or {}).get("state")
        if s:
            return s
    return None


def stuck_working(frames):
    """Working frames that persist after the terminal has been fully silent past
    the stuck-working budget. This ignores legitimate final-turn working/grace
    frames in short captures while still catching a tab stranded in Working."""
    if not frames:
        return None
    return sum(
        1
        for f in frames
        if (f.get("detector") or {}).get("state") == "working"
        and (f.get("quiescence") or {}).get("byte_idle_ms", 0) >= STUCK_WORKING_IDLE_MS
        and (f.get("quiescence") or {}).get("semantic_idle_ms", 0) >= STUCK_WORKING_IDLE_MS
    )


def mid_run_questions(frames):
    """Question frames AFTER the leading contiguous idle/question startup region.
    The agent splash / model-picker is a legitimate pre-turn Question surface and
    lives in that leading region; a Question that appears once the turn is under
    way (mixed with working) is the false-positive we want to catch."""
    startup_end = 0
    for f in frames:
        if (f.get("detector") or {}).get("state") in ("idle", "question"):
            startup_end = f["t_ms"]
        else:
            break
    return sum(1 for f in frames if (f.get("detector") or {}).get("state") == "question" and f["t_ms"] > startup_end)


def find(pattern):
    hits = sorted(glob.glob(str(REPLAYS / pattern)))
    return hits


CHECKS = []


def check(name):
    def deco(fn):
        CHECKS.append((name, fn))
        return fn
    return deco


@check("idle-cli-no-working")
def c_idle(results):
    """Idle terminal/CLI -> no Working indicator (even under focus churn)."""
    out = []
    for p in find("*/idle_focus_churn_v1-*/timeline.json"):
        d, fr = load(p)
        stuck = stuck_working(fr)
        cards = field_frames(fr, "card")
        ok = stuck == 0 and cards == 0
        out.append((ok, f"{Path(p).parent.name[:46]}", f"stuck_working={stuck} card={cards} frames={len(fr)}"))
    return out


@check("question-banner-no-card")
def c_question_no_card(results):
    """Blocking terminal question -> Question state present, ZERO inline cards."""
    out = []
    for p in find("*/terminal_questions_choice_v1-*/timeline.json"):
        d, fr = load(p)
        q = state_counts(fr).get("question", 0)
        cards = field_frames(fr, "card")
        picker = field_frames(fr, "native_picker")
        # Hard rule (runbook #4): PTY providers never emit an inline card/picker.
        # question>0 is coverage info (warn if a capture missed the surface).
        ok = cards == 0 and picker == 0
        warn = "" if q > 0 else "  [WARN: no question frames captured]"
        out.append((ok, f"{Path(p).parent.name[:46]}", f"question={q} card={cards} native_picker={picker}{warn}"))
    return out


@check("normal-answer-ends-idle")
def c_ends_idle(results):
    """Normal answer completion -> Idle. Final frame idle AND no stuck Working
    after full silence. Directly covers Codex/OpenCode stuck-Working P0s."""
    out = []
    pats = [
        "*/provider_cli_question_probe_v1-*/timeline.json",
        "codex/codex_prompt_idle_regression_v1-*/timeline.json",
        "*/systems_design_long_chat_idle_v1-*/timeline.json",
    ]
    seen = set()
    for pat in pats:
        for p in find(pat):
            if p in seen:
                continue
            seen.add(p)
            d, fr = load(p)
            fin = final_state(fr)
            stuck = stuck_working(fr)
            ok = fin == "idle" and (stuck == 0 or stuck is None)
            out.append((ok, f"{Path(p).parent.name[:46]}", f"final={fin} stuck_working={stuck}"))
    return out


@check("working-only-no-question")
def c_working_no_q(results):
    """Long working scenario (no question asked) -> no spurious Question frames."""
    out = []
    for p in find("*/snake_incremental_build_browser*/timeline.json"):
        d, fr = load(p)
        midq = mid_run_questions(fr)
        w = state_counts(fr).get("working", 0)
        ok = midq == 0
        out.append((ok, f"{Path(p).parent.name[:46]}", f"working={w} mid_build_question={midq}"))
    return out


LIVE_ONLY = [
    "stop-cancel-esc-forces-idle  (needs live daemon + key injection oracle)",
    "codex-chatui-message-ordering (needs live ChatUI DOM oracle)",
    "negative-prose-A/B/C-not-question (codex prose answer; live + Twain-derived replay)",
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    report = {}
    all_ok = True
    for name, fn in CHECKS:
        rows = fn(report)
        passed = all(r[0] for r in rows) and len(rows) > 0
        all_ok = all_ok and passed
        report[name] = {"passed": passed, "rows": [{"ok": r[0], "case": r[1], "detail": r[2]} for r in rows]}

    if args.json:
        print(json.dumps({"all_ok": all_ok, "checks": report, "live_only": LIVE_ONLY}, indent=2, ensure_ascii=False))
        return 0 if all_ok else 1

    for name, res in report.items():
        tag = "PASS" if res["passed"] else "FAIL"
        print(f"[{tag}] {name}")
        for r in res["rows"]:
            mark = "  ok " if r["ok"] else "  XX "
            print(f"{mark}{r['case']:48} {r['detail']}")
    print("\nLIVE-ONLY (not asserted here):")
    for x in LIVE_ONLY:
        print(f"  - {x}")
    print(f"\nGATE: {'GREEN' if all_ok else 'RED'}")
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
