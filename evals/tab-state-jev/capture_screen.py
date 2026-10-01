#!/usr/bin/env python3
"""Read-only dev screen capture for the tab-state set; a human assigns every label."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any

from prepare_fixtures import LABELS, LIVE_INPUT, ROOT, regex_scrub


PENDING = Path(__file__).with_name("pending-captures.jsonl")
LIB = ROOT / "evals" / "_lib" / "lib.sh"
CAPTURE_SOURCE = "dev-capture: dc tab get-text"


def dc_json(*args: str) -> Any:
    result = subprocess.run(
        ["bash", "-c", 'source "$1"; shift; dc "$@"', "capture_screen", str(LIB), *args, "--json"],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(f"dc {' '.join(args)} failed ({result.returncode}): {result.stderr.strip()}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise SystemExit(f"dc {' '.join(args)} returned invalid JSON: {error.msg}") from error


def first_value(payload: Any, keys: tuple[str, ...]) -> Any:
    if isinstance(payload, list):
        payload = payload[0] if payload else {}
    if isinstance(payload, dict) and isinstance(payload.get("tab"), dict):
        payload = payload["tab"]
    if not isinstance(payload, dict):
        return None
    for key in keys:
        value = payload.get(key)
        if value not in (None, ""):
            return value
    return None


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8", newline="\n") as output:
        for row in rows:
            output.write(json.dumps(row, ensure_ascii=False) + "\n")


def known_ids() -> set[str]:
    return {row["id"] for row in read_jsonl(PENDING) + read_jsonl(LIVE_INPUT)}


def capture(args: argparse.Namespace) -> int:
    screen_payload = dc_json("tab", "get-text", "--tab", args.tab)
    text = first_value(screen_payload, ("text", "screen", "output"))
    if not isinstance(text, str) or not text.strip():
        raise SystemExit(f"tab {args.tab} returned no screen text")
    inspect = dc_json("tab", "inspect", args.tab)
    provider = args.provider or first_value(inspect, ("agent_type", "provider")) or "unknown"
    boot_stage = first_value(inspect, ("boot_stage",))
    taken = known_ids()
    base = f"live-{provider}-{args.tab[:8]}"
    sample_id = next(f"{base}-{n}" for n in range(1, 10_000) if f"{base}-{n}" not in taken)
    screen = "\n".join(regex_scrub(text.replace("\r\n", "\n").replace("\r", "\n")).rstrip().splitlines()[-40:])
    record = {
        "id": sample_id,
        "provider": provider,
        "boot_stage": boot_stage if isinstance(boot_stage, str) else None,
        "boot_stage_source": "dev tab inspect at capture" if isinstance(boot_stage, str) else None,
        "label": None,
        "source": CAPTURE_SOURCE,
        "note": args.note,
        "screen": screen,
    }
    write_jsonl(PENDING, read_jsonl(PENDING) + [record])
    print(f"captured {sample_id} (unlabelled) provider={provider} boot_stage={record['boot_stage']}")
    print(screen)
    return 0


def pending(_: argparse.Namespace) -> int:
    for row in read_jsonl(PENDING):
        tail = row["screen"].splitlines()[-3:]
        print(f"{row['id']}  boot_stage={row['boot_stage']}  note={row.get('note') or ''}")
        for line in tail:
            print(f"    {line}")
    return 0


def label(args: argparse.Namespace) -> int:
    if args.label not in LABELS:
        raise SystemExit(f"unknown label {args.label!r}; choose one of {sorted(LABELS)}")
    rows = read_jsonl(PENDING)
    match = [row for row in rows if row["id"] == args.id]
    if not match:
        raise SystemExit(f"no pending capture {args.id}")
    record = dict(match[0], label=args.label, source=f"{match[0]['source']} (labelled by {args.by})")
    write_jsonl(LIVE_INPUT, read_jsonl(LIVE_INPUT) + [record])
    write_jsonl(PENDING, [row for row in rows if row["id"] != args.id])
    print(f"{args.id} -> {args.label} in {LIVE_INPUT.name}")
    return 0


def drop(args: argparse.Namespace) -> int:
    rows = read_jsonl(PENDING)
    kept = [row for row in rows if row["id"] != args.id]
    if len(kept) == len(rows):
        raise SystemExit(f"no pending capture {args.id}")
    write_jsonl(PENDING, kept)
    print(f"dropped {args.id}")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    take = commands.add_parser("capture", help="read one dev tab screen into the pending set")
    take.add_argument("--tab", required=True)
    take.add_argument("--provider", help="override the provider read from tab inspect")
    take.add_argument("--note", help="what the capturer saw or triggered")
    take.set_defaults(run=capture)
    commands.add_parser("pending", help="list unlabelled captures").set_defaults(run=pending)
    assign = commands.add_parser("label", help="label a pending capture after reading it")
    assign.add_argument("--id", required=True)
    assign.add_argument("--label", required=True)
    assign.add_argument("--by", required=True, help="who read the screen and chose the label")
    assign.set_defaults(run=label)
    reject = commands.add_parser("drop", help="discard a pending capture")
    reject.add_argument("--id", required=True)
    reject.set_defaults(run=drop)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    return args.run(args)


if __name__ == "__main__":
    sys.exit(main())
