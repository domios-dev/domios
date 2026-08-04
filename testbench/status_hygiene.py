#!/usr/bin/env python3
"""Structural status-hygiene gate for detection replays.

Provider-agnostic. Inspects the *content* of `detector.status_line` and
`detector.last_turn_summary` across timeline frames and flags anomalies that
mean terminal chrome bled into what should be a clean human status string:

  - box_run     : run of box-drawing chars (terminal border / divider bleed)
  - block_run   : run of block/shade chars (progress-bar or tabl bleed)
  - control     : raw C0 control bytes (incl. NUL / BEL) in the text
  - ansi        : raw ANSI/CSI/OSC escape sequence survived into the status
  - path_only   : the whole status is just a filesystem path (no real message)
  - too_long    : status text far longer than any real one-line status

These are STRUCTURAL, never literal-string, checks — vendors change copy
without notice, but none of them legitimately put a 6-char box-drawing run or a
raw ESC byte into a status line. The motivating regression was a Claude
status_line of "Creating… (4s · ↓64 tokens)────────────…" whose divider tail
leaked past extraction; the replay gates counted it as a valid status frame and
missed it. This module is the gate that would have caught it.

Usage:
  status_hygiene.py <timeline.json | replay_dir> [...]   # scan, exit 1 on any
  status_hygiene.py --corpus [ROOT]                       # scan every timeline
  status_hygiene.py --self-test                           # built-in fixtures
"""
import argparse
import json
import sys
import unicodedata
from pathlib import Path

# A real one-line status caps well under 80 chars in production
# (STATUS_MESSAGE_MAX_CHARS = 80, plus the trailing ellipsis the compactor adds).
# Anything materially past that means the compactor never ran / chrome bled in.
MAX_STATUS_CHARS = 100
# Shortest run of box/block chars we treat as chrome bleed. Matches the
# production status tail stripper.
MIN_BLEED_RUN = 3

# Box Drawing (U+2500-257F) + a few stragglers production already strips.
_BOX = set(range(0x2500, 0x2580)) | {0x2014, 0x2015}  # em dash / horizontal bar
# Block Elements + shade blocks (U+2580-259F).
_BLOCK = set(range(0x2580, 0x25A0))


def _max_run(text, charset):
    best = run = 0
    for ch in text:
        if ord(ch) in charset:
            run += 1
            best = max(best, run)
        else:
            run = 0
    return best


def _has_control(text):
    for ch in text:
        o = ord(ch)
        # C0 controls except TAB; DEL; C1 controls. Newlines/CR also illegal in
        # a single-line status, so they count too.
        if o < 0x20 and o != 0x09:
            return True
        if o == 0x7F or 0x80 <= o <= 0x9F:
            return True
    return False


def _is_path_only(text):
    t = text.strip()
    if not t:
        return False
    if not (t.startswith("/") or t.startswith("~/") or t.startswith("./") or t.startswith("../")):
        return False
    # A real status may mention a path mid-sentence ("Editing /x/y"); flag only
    # when the entire status IS a path token — a leaked cwd footer, not a message.
    return " " not in t.strip()


def check_status_text(text):
    """Return a list of structural violation codes for one status string."""
    if not text or not isinstance(text, str):
        return []
    issues = []
    if "\x1b" in text or "\x9b" in text:
        issues.append("ansi")
    if _has_control(text):
        issues.append("control")
    if _max_run(text, _BOX) >= MIN_BLEED_RUN:
        issues.append("box_run")
    if _max_run(text, _BLOCK) >= MIN_BLEED_RUN:
        issues.append("block_run")
    if _is_path_only(text):
        issues.append("path_only")
    if len(text) > MAX_STATUS_CHARS:
        issues.append("too_long")
    return issues


def _detector(frame):
    return frame.get("detector") or frame.get("agent_state") or {}


def _sample(text, n=70):
    safe = "".join(
        ch if (ch.isprintable() and not unicodedata.category(ch).startswith("C")) else f"\\x{ord(ch):02x}"
        for ch in text[:n]
    )
    return safe + ("…" if len(text) > n else "")


def scan_frames(frames):
    """Scan a frame list. Return list of dicts: one per (frame, field) anomaly,
    deduped to the FIRST frame a given (field, text) anomaly appears at."""
    seen = set()
    out = []
    for frame in frames:
        det = _detector(frame)
        for field in ("status_line", "last_turn_summary"):
            text = det.get(field) or frame.get(field)
            if not isinstance(text, str) or not text:
                continue
            issues = check_status_text(text)
            if not issues:
                continue
            key = (field, text, tuple(issues))
            if key in seen:
                continue
            seen.add(key)
            out.append(
                {
                    "frame_index": frame.get("frame_index"),
                    "t_ms": frame.get("t_ms"),
                    "field": field,
                    "issues": issues,
                    "sample": _sample(text),
                }
            )
    return out


def _resolve_timeline(path):
    if path.is_dir():
        return path / "timeline.json"
    return path


def scan_path(path):
    timeline = _resolve_timeline(path)
    if not timeline.exists():
        return None, f"no timeline.json at {timeline}"
    try:
        doc = json.loads(timeline.read_text())
    except Exception as exc:  # noqa: BLE001
        return None, f"unreadable timeline {timeline}: {exc}"
    frames = doc.get("frames") or []
    return scan_frames(frames), None


SELF_TEST_CASES = [
    # (text, expected_issues_subset, description)
    ("Creating… (4s · ↓64 tokens)", [], "clean Claude widget status"),
    ("Build · DeepSeek V4 Flash Free", [], "clean OpenCode status"),
    ("Editing /etc/hosts now", [], "path mid-sentence is fine"),
    ("Composing", [], "single-word status"),
    ("Creating… (4s · ↓64 tokens)────────────────────────", ["box_run"], "divider tail bleed"),
    ("Working ━━━━━━━━", ["box_run"], "heavy horizontal bleed"),
    ("Build ████████ 80%", ["block_run"], "progress block bleed"),
    ("\x1b[2mthinking\x1b[0m", ["ansi", "control"], "raw ANSI escape survived"),
    ("done\x07", ["control"], "BEL control byte"),
    ("/Users/kandrey/Developer/codeterm-alpha/testbench", ["path_only"], "leaked cwd as status"),
    ("~/scratch/snake.html", ["path_only"], "leaked home path as status"),
    ("x" * 140, ["too_long"], "absurdly long status"),
]


def self_test():
    failures = 0
    for text, expected, desc in SELF_TEST_CASES:
        got = check_status_text(text)
        if expected:
            ok = all(code in got for code in expected)
        else:
            ok = got == []
        status = "ok  " if ok else "FAIL"
        if not ok:
            failures += 1
        print(f"  [{status}] {desc}: got={got} expected⊇{expected}")
    print(f"\nself-test: {len(SELF_TEST_CASES) - failures}/{len(SELF_TEST_CASES)} passed")
    return 1 if failures else 0


NEGATIVE_FIXTURE = Path(__file__).resolve().parent / "fixtures" / "status-hygiene" / "divider-bleed.timeline.json"
# Each entry: (frame_index, field, required-issue). Proves the gate fires
# end-to-end on a real timeline.json, not just on inline strings.
NEGATIVE_EXPECT = [
    (1, "status_line", "box_run"),
    (2, "status_line", "ansi"),
    (3, "last_turn_summary", "path_only"),
]


def self_proof():
    anomalies, err = scan_path(NEGATIVE_FIXTURE)
    if err:
        print(f"FAIL negative fixture unreadable: {err}")
        return 1
    found = {(a["frame_index"], a["field"]): a["issues"] for a in anomalies}
    failures = 0
    for frame_index, field, code in NEGATIVE_EXPECT:
        got = found.get((frame_index, field), [])
        ok = code in got
        if not ok:
            failures += 1
        print(f"  [{'ok  ' if ok else 'FAIL'}] frame {frame_index} {field}: expected {code!r} in {got}")
    print(f"\nself-proof: {len(NEGATIVE_EXPECT) - failures}/{len(NEGATIVE_EXPECT)} negative cases caught")
    return 1 if failures else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("targets", nargs="*", type=Path, help="timeline.json files or replay dirs")
    parser.add_argument("--corpus", nargs="?", const="testbench/replays", help="scan every timeline.json under ROOT")
    parser.add_argument("--quiet", action="store_true", help="print only anomalies and the final summary")
    parser.add_argument("--self-test", action="store_true", help="unit-check check_status_text on inline cases")
    parser.add_argument("--self-proof", action="store_true", help="prove the gate fires on the bad timeline fixture")
    args = parser.parse_args()

    if args.self_test:
        return self_test()
    if args.self_proof:
        return self_proof()

    targets = list(args.targets)
    if args.corpus:
        root = Path(args.corpus)
        targets += sorted(root.rglob("timeline.json"))
    if not targets:
        parser.error("pass timeline/replay targets, --corpus, or --self-test")

    total_anomalies = 0
    scanned = 0
    for path in targets:
        anomalies, err = scan_path(path)
        if err:
            print(f"SKIP {path}: {err}")
            continue
        scanned += 1
        if anomalies:
            total_anomalies += len(anomalies)
            print(f"BAD {path}: {len(anomalies)} status-hygiene anomaly(ies)")
            for a in anomalies:
                print(f"    frame={a['frame_index']} t_ms={a['t_ms']} {a['field']} {a['issues']}: {a['sample']!r}")
        elif not args.quiet:
            print(f"OK  {path}")

    print(f"\nscanned {scanned} timeline(s), {total_anomalies} anomaly(ies)")
    return 1 if total_anomalies else 0


if __name__ == "__main__":
    sys.exit(main())
