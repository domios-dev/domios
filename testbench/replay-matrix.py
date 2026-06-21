#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PROVIDERS = ["claude", "opencode", "codex", "cursor"]
DEFAULT_SCENARIOS = [
    "testbench/detect-scenarios/systems_design_long_chat_idle_v1.yaml",
    "testbench/detect-scenarios/idle_focus_churn_v1.yaml",
]
SCENARIO_DIR = ROOT / "testbench" / "detect-scenarios"


def parse_geometry(value: str) -> tuple[int, int]:
    parts = value.lower().split("x", 1)
    if len(parts) != 2:
        raise argparse.ArgumentTypeError("geometry must be COLSxROWS, for example 120x32")
    try:
        cols = int(parts[0])
        rows = int(parts[1])
    except ValueError as exc:
        raise argparse.ArgumentTypeError("geometry must use integer cols and rows") from exc
    if cols < 40 or rows < 10:
        raise argparse.ArgumentTypeError("geometry is too small for agent replay capture")
    return cols, rows


def run(cmd: list[str], timeout: int | None = None) -> tuple[int, str]:
    proc = subprocess.run(
        cmd,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=timeout,
        check=False,
    )
    return proc.returncode, proc.stdout


def resolve_scenario(value: str) -> Path:
    path = Path(value)
    if path.exists():
        return path
    if path.suffix != ".yaml":
        candidate = SCENARIO_DIR / f"{value}.yaml"
        if candidate.exists():
            return candidate
    candidate = SCENARIO_DIR / path.name
    if candidate.exists():
        return candidate
    return path


def render_and_gate(replay_dir: Path) -> dict[str, object]:
    timeline = replay_dir / "timeline.json"
    raw = replay_dir / "raw.cast"
    meta = replay_dir / "meta.json"
    phases = replay_dir / "phases.json"
    renderer = ROOT / "src-tauri" / "target" / "debug" / "examples" / "render_timeline"
    if not renderer.exists():
        code, out = run(["cargo", "build", "--example", "render_timeline"], timeout=180)
        if code != 0:
            return {"ok": False, "stage": "build_renderer", "output": out}
    code, out = run(
        [
            str(renderer),
            str(raw),
            str(timeline),
            "--sample-ms",
            "75",
            "--meta",
            str(meta),
            "--phases",
            str(phases),
        ],
        timeout=900,
    )
    if code != 0:
        return {"ok": False, "stage": "render", "output": out}
    code, analyze = run(["python3", "testbench/replay-analyze.py", str(timeline)], timeout=120)
    code2, doctor = run(
        [
            "python3",
            "testbench/replay-doctor.py",
            str(replay_dir),
        ],
        timeout=900,
    )
    return {
        "ok": code == 0 and code2 == 0,
        "stage": "gate",
        "render": out,
        "analyze": analyze,
        "doctor": doctor,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Capture/render/gate the detection replay matrix.")
    parser.add_argument("--provider", action="append", choices=DEFAULT_PROVIDERS)
    parser.add_argument("--scenario", action="append")
    parser.add_argument("--run-prefix", default=time.strftime("%Y%m%d-%H%M%S"))
    parser.add_argument(
        "--geometry",
        action="append",
        type=parse_geometry,
        help="Real capture geometry as COLSxROWS. Repeat to record the same scenario at multiple PTY sizes.",
    )
    parser.add_argument("--skip-capture", action="store_true")
    args = parser.parse_args()

    providers = args.provider or DEFAULT_PROVIDERS
    scenarios = [resolve_scenario(s) for s in (args.scenario or DEFAULT_SCENARIOS)]
    geometries = args.geometry or [None]
    results = []
    for scenario in scenarios:
        scenario_id = scenario.stem
        for provider in providers:
            for geometry in geometries:
                geom_suffix = "" if geometry is None else f"-{geometry[0]}x{geometry[1]}"
                run_id = f"{args.run_prefix}-{provider}-{scenario_id}{geom_suffix}"
                record = {
                    "provider": provider,
                    "scenario": scenario_id,
                    "run_id": run_id,
                    "geometry": None if geometry is None else {"cols": geometry[0], "rows": geometry[1]},
                }
                if not args.skip_capture:
                    capture_cmd = [
                            "python3",
                            "testbench/replay-capture.py",
                            str(scenario),
                            "--provider",
                            provider,
                            "--run-id",
                            run_id,
                    ]
                    if geometry is not None:
                        capture_cmd.extend(["--cols", str(geometry[0]), "--rows", str(geometry[1])])
                    code, out = run(capture_cmd, timeout=1800)
                    record["capture_output"] = out
                    if code != 0:
                        record.update({"ok": False, "stage": "capture", "code": code})
                        results.append(record)
                        print(json.dumps(record, ensure_ascii=False), flush=True)
                        continue
                    replay_dir = Path(out.strip().splitlines()[-1])
                else:
                    replay_dir = ROOT / "testbench" / "replays" / provider / f"{scenario_id}-{run_id}"
                record["replay_dir"] = str(replay_dir)
                record["gate"] = render_and_gate(replay_dir)
                record["ok"] = bool(record["gate"].get("ok"))
                results.append(record)
                print(json.dumps(record, ensure_ascii=False), flush=True)

    out_path = ROOT / "testbench" / "replays" / f"matrix-{args.run_prefix}.json"
    out_path.write_text(json.dumps(results, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(out_path)
    return 0 if all(r.get("ok") for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
