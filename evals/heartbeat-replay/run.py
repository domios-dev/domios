#!/usr/bin/env python3
"""Replay reconstructed daemon snapshots through the production Rust gate."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", required=True)
    parser.add_argument("--arm", choices=["fixture", "no-model"], required=True)
    parser.add_argument("--input", type=Path, default=Path(__file__).with_name("reconstructed.json"))
    args = parser.parse_args()
    fixture = json.loads(args.input.read_text())
    if fixture["plan_id"] != args.plan or not fixture["ticks"]:
        parser.error("plan mismatch or empty replay")
    root = Path(__file__).resolve().parents[2]
    suffix = args.arm.replace("-", "_")
    command = [str(root / "build.sh"), "test-one", "app", f"heartbeat_gate_replay_{suffix}"]
    env = dict(os.environ, CODETERM_HEARTBEAT_REPLAY_INPUT=str(args.input.resolve()), RUST_TEST_NOCAPTURE="1")
    result = subprocess.run(command, cwd=root, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    print(result.stdout, end="")
    if result.returncode:
        return result.returncode
    marker = "HEARTBEAT_REPLAY_RESULT="
    summaries = [json.loads(line.split(marker, 1)[1]) for line in result.stdout.splitlines() if marker in line]
    if len(summaries) != 1 or summaries[0]["ticks"] == 0:
        print("Replay failed: no unique non-empty result from the production gate", file=sys.stderr)
        return 1
    summary = summaries[0]
    summary["evidence"] = fixture["evidence"]
    summary["qualification"] = "conservative reconstruction; Choice approval required, hard incidents never force a wake; no measured savings or model quality"
    print(json.dumps(summary, indent=2))
    return 1 if summary["missed_incidents"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
