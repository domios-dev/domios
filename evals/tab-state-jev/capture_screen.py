#!/usr/bin/env python3
"""Capture dev tab text and let a human assign its state label."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import subprocess
import sys
import uuid
from pathlib import Path
from typing import Any

from prepare_fixtures import LABELS, LIVE_INPUT, ROOT, regex_scrub


HERE = Path(__file__).resolve().parent
PENDING = HERE / "pending-captures.jsonl"
CAPTURE_STATE = HERE / "capture-state.json"
LIB = ROOT / "evals" / "_lib" / "lib.sh"
CAPTURE_SOURCE = "dev-capture: dc tab get-text"
CONTEXT_INPUT_FIELDS = {
    "requested_model",
    "session_model",
    "role",
    "plan_id",
    "task_id",
    "spawned_by_tab",
    "seconds_since_spawn",
    "seconds_since_last_output",
    "structural_verdict",
    "pending_prompt_kind",
}


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


def nested_objects(value: Any):
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from nested_objects(child)
    elif isinstance(value, list):
        for child in value:
            yield from nested_objects(child)


def tab_record(payload: Any, tab_id: str) -> dict[str, Any]:
    for item in nested_objects(payload):
        if item.get("id") == tab_id and ("agent_type" in item or "backing" in item):
            return item
    raise SystemExit(f"tab inspect did not return tab {tab_id}")


def provider_descriptor(payload: Any, provider_id: str) -> dict[str, str]:
    for item in nested_objects(payload):
        if item.get("id") == provider_id and "transport" in item:
            transport = item.get("transport")
            display_name = item.get("display_name")
            if transport not in {"pty", "chat_backend"} or not isinstance(display_name, str):
                break
            return {"transport": transport, "display_name": display_name}
    raise SystemExit(
        f"provider catalog has no capability descriptor for the tab's provider {provider_id!r}"
    )


def provider_id_for_tab(tab: dict[str, Any], override: str | None) -> str:
    provider_id = tab.get("agent_type")
    if isinstance(provider_id, str) and provider_id:
        return provider_id
    if isinstance(override, str) and override:
        return override
    raise SystemExit("tab has no provider identity; pass --provider for a provider CLI shell")


def first_value(payload: Any, keys: tuple[str, ...]) -> Any:
    for item in nested_objects(payload):
        for key in keys:
            value = item.get(key)
            if value not in (None, ""):
                return value
    return None


def read_json(path: Path, fallback: Any) -> Any:
    if not path.exists():
        return fallback
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"could not read {path.name}: {error}") from error


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    try:
        return [
            json.loads(line)
            for line in path.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"could not read {path.name}: {error}") from error


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8", newline="\n") as output:
        for row in rows:
            output.write(json.dumps(row, ensure_ascii=False) + "\n")


def read_context(path: Path) -> dict[str, Any]:
    try:
        context = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"context file is unreadable or invalid: {error}") from error
    if not isinstance(context, dict) or not CONTEXT_INPUT_FIELDS <= context.keys():
        raise SystemExit(f"context file must include: {', '.join(sorted(CONTEXT_INPUT_FIELDS))}")
    for field in ("seconds_since_spawn", "seconds_since_last_output"):
        value = context[field]
        if (
            isinstance(value, bool)
            or not isinstance(value, (int, float))
            or not math.isfinite(value)
            or value < 0
        ):
            raise SystemExit(f"context field {field} must be a nonnegative number")
    return context


def known_ids() -> set[str]:
    return {row["id"] for row in read_jsonl(PENDING) + read_jsonl(LIVE_INPUT)}


def capture(args: argparse.Namespace) -> int:
    screen_payload = dc_json("tab", "get-text", "--tab", args.tab)
    text = first_value(screen_payload, ("text", "screen", "output"))
    if not isinstance(text, str) or not text.strip():
        raise SystemExit(f"tab {args.tab} returned no screen text")
    inspected = dc_json("tab", "inspect", "--tab", args.tab)
    tab = tab_record(inspected, args.tab)
    provider_id = provider_id_for_tab(tab, args.provider)
    capability = provider_descriptor(dc_json("agent", "providers"), provider_id)
    context = read_context(args.context_file)
    screen = "\n".join(regex_scrub(text.replace("\r\n", "\n").replace("\r", "\n")).rstrip().splitlines()[-40:])
    state = read_json(CAPTURE_STATE, {})
    previous_hash = state.get(args.tab) if isinstance(state, dict) else None
    taken = known_ids()
    sample_id = f"live-{provider_id}-{uuid.uuid4().hex[:10]}"
    while sample_id in taken:
        sample_id = f"live-{provider_id}-{uuid.uuid4().hex[:10]}"
    row = {
        "id": sample_id,
        "provider": provider_id,
        "transport": capability["transport"],
        "provider_capability": capability,
        "boot_stage": first_value(tab, ("boot_stage",)),
        "label": None,
        "labelled_by": None,
        "source": CAPTURE_SOURCE,
        "note": args.note,
        "screen": screen,
        "context_input": context,
        "capture_tab_id": args.tab,
        "previous_screen_hash": previous_hash,
        "screen_hash": hashlib.sha256(text.encode("utf-8")).hexdigest(),
    }
    write_jsonl(PENDING, read_jsonl(PENDING) + [row])
    print(f"captured {sample_id} (unlabelled) transport={capability['transport']}")
    print(screen)
    return 0


def pending(_: argparse.Namespace) -> int:
    for row in read_jsonl(PENDING):
        print(f"{row['id']}  provider={row['provider']}  transport={row['transport']}  note={row.get('note') or ''}")
        for line in row["screen"].splitlines()[-3:]:
            print(f"    {line}")
    return 0


def label(args: argparse.Namespace) -> int:
    if args.label not in LABELS:
        raise SystemExit(f"unknown label {args.label!r}; choose one of {sorted(LABELS)}")
    if not args.by.strip():
        raise SystemExit("a human labeler name is required")
    rows = read_jsonl(PENDING)
    match = next((row for row in rows if row["id"] == args.id), None)
    if match is None:
        raise SystemExit(f"no pending capture {args.id}")
    tab_id = match.pop("capture_tab_id")
    previous_hash = match.pop("previous_screen_hash")
    screen_hash = match.pop("screen_hash")
    context = match.pop("context_input")
    context["provider_capability"] = match["provider_capability"]
    context["screen_changed"] = previous_hash is not None and previous_hash != screen_hash
    record = dict(
        match,
        label=args.label,
        labelled_by=args.by.strip(),
        context=context,
    )
    record["source"] = f"{CAPTURE_SOURCE}; human-labelled by {args.by.strip()}"
    write_jsonl(LIVE_INPUT, read_jsonl(LIVE_INPUT) + [record])
    write_jsonl(PENDING, [row for row in rows if row["id"] != args.id])
    state = read_json(CAPTURE_STATE, {})
    if not isinstance(state, dict):
        state = {}
    state[tab_id] = screen_hash
    CAPTURE_STATE.write_text(json.dumps(state, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"{args.id} labelled in {LIVE_INPUT.name}")
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
    take = commands.add_parser("capture", help="read one dev tab into the unlabelled set")
    take.add_argument("--tab", required=True)
    take.add_argument("--context-file", type=Path, required=True)
    take.add_argument("--provider", help="capability lookup for a provider CLI in a plain shell tab")
    take.add_argument("--note", help="reproduction and what the capturer saw")
    take.set_defaults(run=capture)
    commands.add_parser("pending", help="list captures awaiting human labels").set_defaults(run=pending)
    assign = commands.add_parser("label", help="assign a label after a human reads the screen")
    assign.add_argument("--id", required=True)
    assign.add_argument("--label", required=True)
    assign.add_argument("--by", required=True)
    assign.set_defaults(run=label)
    reject = commands.add_parser("drop", help="discard an unlabelled capture")
    reject.add_argument("--id", required=True)
    reject.set_defaults(run=drop)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    return args.run(args)


if __name__ == "__main__":
    sys.exit(main())
