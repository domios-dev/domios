#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from provider_rows import rows_for_replay  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
GEOMETRIES = [(32, 120), (30, 100), (40, 160), (50, 120)]


def run_render(replay_dir: Path, rows: int, cols: int, sample_ms: int) -> dict:
    renderer = ROOT / "src-tauri" / "target" / "debug" / "examples" / "render_timeline"
    with tempfile.TemporaryDirectory(prefix="ct-geo-") as td:
        out = Path(td) / "timeline.json"
        subprocess.run(
            [
                str(renderer),
                str(replay_dir / "raw.cast"),
                str(out),
                "--sample-ms",
                str(sample_ms),
                "--meta",
                str(replay_dir / "meta.json"),
                "--phases",
                str(replay_dir / "phases.json"),
                "--rows",
                str(rows),
                "--cols",
                str(cols),
                "--status-bar-rows",
                str(rows_for_replay(replay_dir, fixture_rows=0)),
            ],
            cwd=ROOT,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        return json.loads(out.read_text())


def signature(doc: dict) -> dict:
    frames = doc.get("frames", [])
    states = {}
    transitions = []
    prev = None
    for frame in frames:
        det = frame.get("detector") or {}
        state = det.get("state")
        states[state] = states.get(state, 0) + 1
        if state != prev:
            transitions.append([frame.get("t_ms"), prev, state])
            prev = state
    return {
        "frames": len(frames),
        "states": states,
        "cards": sum(1 for f in frames if (f.get("detector") or {}).get("card")),
        "status": sum(1 for f in frames if (f.get("detector") or {}).get("status_line")),
        "transitions": len(transitions),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Compare replay detector invariants across terminal geometries.")
    parser.add_argument("replay_dir", type=Path)
    parser.add_argument("--sample-ms", type=int, default=250)
    args = parser.parse_args()

    results = []
    for rows, cols in GEOMETRIES:
        doc = run_render(args.replay_dir, rows, cols, args.sample_ms)
        sig = signature(doc)
        sig["rows"] = rows
        sig["cols"] = cols
        results.append(sig)
        print(json.dumps(sig, ensure_ascii=False))

    baseline = {k: v for k, v in results[0].items() if k not in {"rows", "cols", "frames", "status"}}
    failures = []
    for result in results[1:]:
        comparable = {k: v for k, v in result.items() if k not in {"rows", "cols", "frames", "status"}}
        if comparable != baseline:
            failures.append({"geometry": [result["rows"], result["cols"]], "expected": baseline, "got": comparable})
    if failures:
        print("BAD geometry-sensitive detector output")
        print(json.dumps(failures, indent=2, ensure_ascii=False))
        return 1
    print("OK geometry-invariant detector signature")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
