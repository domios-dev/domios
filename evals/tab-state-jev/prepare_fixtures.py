#!/usr/bin/env python3
"""Sanitize and assemble human-labelled dev captures for the tab-state eval."""

from __future__ import annotations

import json
import math
import re
from collections import Counter
from pathlib import Path
from typing import Any


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
OUT = HERE / "screens.jsonl"
LIVE_INPUT = HERE / "live-captures.jsonl"
LABELS = {
    "ready_idle",
    "working_normally",
    "provider_update_prompt",
    "trust_or_permission_prompt",
    "login_or_auth_required",
    "model_unavailable_or_quota",
    "crashed_to_shell",
    "error_loop",
    "still_starting",
}
CONTEXT_FIELDS = {
    "provider_capability",
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
    "screen_changed",
}
CONTEXT_ID_FIELDS = {"plan_id", "task_id", "spawned_by_tab"}


def regex_scrub(value: str) -> str:
    value = re.sub(
        r"(?i)(?:[A-Z]:\\Users\\[^\\\s]+(?:\\[^\s]*)*|[A-Z]:\\Developer\\[^\s]*|"
        r"~/Developer/[^\s]*|~\\[^\\\s]+(?:\\[^\s]*)*|~/\.codeterm-dev[^\s]*|"
        r"~\\\.codeterm-dev[^\s]*|/(?:home|Users)/[^\s]+|/srv/(?:app|codeterm)[^\s]*)",
        "<workspace>",
        value,
    )
    value = re.sub(
        r"(?i)(?:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|"
        r"\btab=[0-9a-f]{8}\b|\bTab [0-9a-f]{8}\b)",
        "<id>",
        value,
    )
    value = re.sub(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}", "<email>", value)
    value = value.replace("user@host", "agent@host")
    value = value.replace("C:\\Users\\foo", "C:\\Users\\agent")
    value = value.replace("C:\\Users\\User", "C:\\Users\\agent")
    return value


def scrub_context(value: Any) -> Any:
    if isinstance(value, str):
        return regex_scrub(value)
    if isinstance(value, list):
        return [scrub_context(item) for item in value]
    if isinstance(value, dict):
        return {
            key: "<id>" if key in CONTEXT_ID_FIELDS and isinstance(item, str) and item else scrub_context(item)
            for key, item in value.items()
        }
    return value


def read_live_captures() -> list[dict[str, Any]]:
    if not LIVE_INPUT.exists():
        return []
    rows: list[dict[str, Any]] = []
    seen: set[str] = set()
    for line_number, line in enumerate(LIVE_INPUT.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            source = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"invalid JSON in live capture {line_number}: {error.msg}") from error
        required = {
            "id",
            "provider",
            "transport",
            "provider_capability",
            "label",
            "labelled_by",
            "source",
            "screen",
            "context",
        }
        if not isinstance(source, dict) or not required <= source.keys():
            raise ValueError(f"live capture {line_number} is missing required fields")
        if not isinstance(source["id"], str) or not source["id"] or source["id"] in seen:
            raise ValueError(f"live capture {line_number} has a missing or duplicate id")
        seen.add(source["id"])
        if source["label"] not in LABELS:
            raise ValueError(f"live capture {line_number} has an unsupported label")
        if not isinstance(source["labelled_by"], str) or not source["labelled_by"].strip():
            raise ValueError(f"live capture {line_number} has no human labeler")
        if not isinstance(source["source"], str) or not source["source"].startswith("dev-capture"):
            raise ValueError(f"live capture {line_number} is not from the read-only dev capture helper")
        if not isinstance(source["screen"], str) or not source["screen"].strip():
            raise ValueError(f"live capture {line_number} has no screen text")

        capability = source["provider_capability"]
        if (
            not isinstance(capability, dict)
            or not isinstance(capability.get("transport"), str)
            or capability.get("transport") not in {"pty", "chat_backend"}
            or not isinstance(capability.get("display_name"), str)
            or not capability["display_name"].strip()
            or source["transport"] != capability["transport"]
        ):
            raise ValueError(f"live capture {line_number} has no capability-declared transport")
        context = source["context"]
        if not isinstance(context, dict) or not CONTEXT_FIELDS <= context.keys():
            raise ValueError(f"live capture {line_number} is missing runtime context fields")
        if context["provider_capability"] != capability:
            raise ValueError(f"live capture {line_number} context capability differs from its descriptor")
        if not isinstance(context["screen_changed"], bool):
            raise ValueError(f"live capture {line_number} has an invalid screen_changed value")
        for field in ("seconds_since_spawn", "seconds_since_last_output"):
            value = context[field]
            if (
                isinstance(value, bool)
                or not isinstance(value, (int, float))
                or not math.isfinite(value)
                or value < 0
            ):
                raise ValueError(f"live capture {line_number} has an invalid {field}")

        screen = regex_scrub(source["screen"].replace("\r\n", "\n").replace("\r", "\n"))
        row = dict(source)
        row["screen"] = "\n".join(screen.rstrip().splitlines()[-40:])
        row["provider_capability"] = dict(capability)
        row["context"] = scrub_context(context)
        if isinstance(row.get("note"), str):
            row["note"] = regex_scrub(row["note"])
        rows.append(row)
    return rows


def support_report(rows: list[dict[str, Any]]) -> dict[str, Any]:
    providers = sorted({row["provider"] for row in rows})
    transports = sorted({row["transport"] for row in rows})
    return {
        "records": len(rows),
        "per_provider": {
            provider: dict(Counter(row["label"] for row in rows if row["provider"] == provider))
            for provider in providers
        },
        "per_transport": {
            transport: dict(Counter(row["label"] for row in rows if row["transport"] == transport))
            for transport in transports
        },
    }


def main() -> None:
    rows = read_live_captures()
    with OUT.open("w", encoding="utf-8", newline="\n") as output:
        for row in rows:
            output.write(json.dumps(row, ensure_ascii=False) + "\n")
    print(json.dumps(support_report(rows), ensure_ascii=False, indent=2))
    print(f"wrote {len(rows)} human-labelled dev captures to {OUT.name}")


if __name__ == "__main__":
    main()
