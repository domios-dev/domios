"""Task-sidecar model conversion that preserves legacy YAML presentation."""

import json
import pathlib
import re
import secrets
import time
from dataclasses import dataclass

import yaml


AUDIT_MARKER = "explicit legacy parked-to-paused conversion"
TOP_LEVEL_KEY = re.compile(r"^([A-Za-z_][A-Za-z0-9_-]*):")
LIFECYCLE_KEYS = {"status", "park_reason", "pause_reason"}


@dataclass(frozen=True)
class Conversion:
    path: pathlib.Path
    original: bytes
    text: str
    audit_id: str
    timestamp_ms: int


def validate_lifecycle_keys(node: yaml.Node | None, location: str = "document") -> None:
    if isinstance(node, yaml.MappingNode):
        seen: set[str] = set()
        for key_node, value_node in node.value:
            key = key_node.value if isinstance(key_node, yaml.ScalarNode) else None
            if key in LIFECYCLE_KEYS:
                if key in seen:
                    raise ValueError(f"duplicate lifecycle key {key!r} in {location}")
                seen.add(key)
            validate_lifecycle_keys(value_node, f"{location}.{key or '<complex-key>'}")
    elif isinstance(node, yaml.SequenceNode):
        for index, item in enumerate(node.value):
            validate_lifecycle_keys(item, f"{location}[{index}]")


def load_yaml(text: str) -> object:
    validate_lifecycle_keys(yaml.compose(text))
    return yaml.safe_load(text)


def yaml_scalar(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def top_level_index(lines: list[str], key: str) -> int | None:
    prefix = f"{key}:"
    return next((index for index, line in enumerate(lines) if line.startswith(prefix)), None)


def block_end(lines: list[str], start: int) -> int:
    for index in range(start + 1, len(lines)):
        if TOP_LEVEL_KEY.match(lines[index]):
            return index
    return len(lines)


def direct_list_items(lines: list[str], start: int, end: int) -> list[tuple[int, str]]:
    candidates = [
        (index, match.group(1))
        for index in range(start + 1, end)
        if (match := re.match(r"^(\s*)-(?:\s|$)", lines[index]))
    ]
    if not candidates:
        return []
    direct_indent = min(len(indent) for _, indent in candidates)
    return [(index, indent) for index, indent in candidates if len(indent) == direct_indent]


def replace_top_level(lines: list[str], key: str, value: str) -> None:
    index = top_level_index(lines, key)
    if index is None:
        raise ValueError(f"missing top-level {key}")
    comment = ""
    match = re.match(rf"^{re.escape(key)}:\s*[^#\r\n]*(\s+#.*)?(\r?\n)?$", lines[index])
    if match and match.group(1):
        comment = match.group(1)
    newline = "\r\n" if lines[index].endswith("\r\n") else "\n"
    lines[index] = f"{key}: {value}{comment}{newline}"


def set_pause_reason(lines: list[str], reason: str) -> None:
    parked_index = top_level_index(lines, "park_reason")
    scalar = yaml_scalar(reason)
    if parked_index is not None:
        newline = "\r\n" if lines[parked_index].endswith("\r\n") else "\n"
        comment = ""
        match = re.match(r"^park_reason:\s*[^#\r\n]*(\s+#.*)?(?:\r?\n)?$", lines[parked_index])
        if match and match.group(1):
            comment = match.group(1)
        lines[parked_index] = f"pause_reason: {scalar}{comment}{newline}"
        return
    status_index = top_level_index(lines, "status")
    newline = "\r\n" if lines[status_index].endswith("\r\n") else "\n"
    lines.insert(status_index + 1, f"pause_reason: {scalar}{newline}")


def clear_lane_assignment(lines: list[str]) -> None:
    start = top_level_index(lines, "lane")
    if start is None:
        return
    end = block_end(lines, start)
    pane_index = next(
        (index for index in range(start + 1, end) if re.match(r"^  pane_id:", lines[index])),
        None,
    )
    if pane_index is None:
        return
    newline = "\r\n" if lines[pane_index].endswith("\r\n") else "\n"
    lines[pane_index] = f"  pane_id: null{newline}"


def close_active_attempts(lines: list[str], timestamp_ms: int) -> None:
    start = top_level_index(lines, "execution_attempts")
    if start is None:
        return
    end = block_end(lines, start)
    items = direct_list_items(lines, start, end)
    item_starts = [index for index, _ in items]
    for item_start, indent in reversed(items):
        item_end = next((index for index in item_starts if index > item_start), end)
        field_indent = f"{indent}  "
        closed_index = next(
            (
                index
                for index in range(item_start, item_end)
                if re.match(rf"^{re.escape(field_indent)}closed_at:", lines[index])
            ),
            None,
        )
        if closed_index is not None:
            current = yaml.safe_load(lines[closed_index].split(":", 1)[1])
            if current is None:
                newline = "\r\n" if lines[closed_index].endswith("\r\n") else "\n"
                lines[closed_index] = f"{field_indent}closed_at: {timestamp_ms}{newline}"
            continue
        newline = "\r\n" if lines[item_end - 1].endswith("\r\n") else "\n"
        lines.insert(item_end, f"{field_indent}closed_at: {timestamp_ms}{newline}")
        end += 1


def milestone_lines(
    audit_id: str, reason: str, timestamp_ms: int, newline: str, indent: str
) -> list[str]:
    milestone = {
        "id": audit_id,
        "from": {
            "pane_id": "legacy-state-audit",
            "session_id": None,
            "title": "Legacy state audit",
        },
        "kind": "decision",
        "text": f"paused: {reason} ({AUDIT_MARKER})",
        "ts": timestamp_ms,
        "links": [],
        "loop_id": None,
        "iteration": None,
        "canvas": {"x": None, "y": None, "collapsed": False},
        "attachments": [],
    }
    dumped = yaml.safe_dump([milestone], sort_keys=False, allow_unicode=True).splitlines()
    return [f"{indent}{line}{newline}" for line in dumped]


def append_audit_milestone(
    lines: list[str], audit_id: str, pause_reason: str, audit_reason: str, timestamp_ms: int
) -> None:
    start = top_level_index(lines, "milestones")
    if start is None:
        raise ValueError("missing top-level milestones")
    newline = "\r\n" if lines[start].endswith("\r\n") else "\n"
    milestone_reason = f"{pause_reason}; audit reason: {audit_reason}"
    if re.match(r"^milestones:\s*\[\s*\]\s*(?:#.*)?(?:\r?\n)?$", lines[start]):
        comment_match = re.match(
            r"^milestones:\s*\[\s*\]\s*(#.*)?(?:\r?\n)?$", lines[start]
        )
        comment = f" {comment_match.group(1)}" if comment_match and comment_match.group(1) else ""
        lines[start] = f"milestones:{comment}{newline}"
        rendered = milestone_lines(audit_id, milestone_reason, timestamp_ms, newline, "")
        lines[start + 1:start + 1] = rendered
        return
    end = block_end(lines, start)
    items = direct_list_items(lines, start, end)
    if not items:
        raise ValueError("milestones must be an empty or block-style list")
    indent = items[0][1]
    rendered = milestone_lines(audit_id, milestone_reason, timestamp_ms, newline, indent)
    lines[end:end] = rendered


def matching_audit_stride(item: object, pause_reason: str) -> bool:
    if not isinstance(item, dict):
        return False
    source = item.get("from")
    canvas = item.get("canvas")
    text = item.get("text")
    timestamp = item.get("ts")
    prefix = f"paused: {pause_reason}; audit reason: "
    suffix = f" ({AUDIT_MARKER})"
    return (
        isinstance(item.get("id"), str)
        and bool(item["id"].strip())
        and source
        == {
            "pane_id": "legacy-state-audit",
            "session_id": None,
            "title": "Legacy state audit",
        }
        and item.get("kind") == "decision"
        and isinstance(text, str)
        and text.startswith(prefix)
        and text.endswith(suffix)
        and bool(text[len(prefix) : -len(suffix)].strip())
        and isinstance(timestamp, int)
        and not isinstance(timestamp, bool)
        and timestamp > 0
        and item.get("links") == []
        and item.get("loop_id") is None
        and item.get("iteration") is None
        and canvas == {"x": None, "y": None, "collapsed": False}
        and item.get("attachments") == []
    )


def validate_already_converted(value: object) -> bool:
    if not isinstance(value, dict) or value.get("status") != "paused":
        return False
    if "park_reason" in value:
        raise ValueError("paused record retains legacy park_reason")
    pause_reason = value.get("pause_reason")
    if not isinstance(pause_reason, str) or not pause_reason.strip():
        raise ValueError("paused record requires a nonblank pause_reason")
    milestones = value.get("milestones")
    if not isinstance(milestones, list):
        raise ValueError("paused record requires milestone audit history")
    marked = [
        item
        for item in milestones
        if isinstance(item, dict) and AUDIT_MARKER in str(item.get("text", ""))
    ]
    if len(marked) != 1:
        raise ValueError("paused record requires exactly one conversion audit stride")
    if not matching_audit_stride(marked[0], pause_reason):
        raise ValueError("paused record has malformed or mismatched conversion audit stride")
    if sum(item.get("id") == marked[0]["id"] for item in milestones if isinstance(item, dict)) != 1:
        raise ValueError("paused record conversion audit id is not unique")
    return True


def audit_metadata(value: dict) -> tuple[set[str], int]:
    ids: set[str] = set()
    timestamps = [0]
    for milestone in value.get("milestones", []):
        if not isinstance(milestone, dict):
            continue
        milestone_id = milestone.get("id")
        if isinstance(milestone_id, str):
            ids.add(milestone_id)
        timestamp = milestone.get("ts")
        if isinstance(timestamp, int):
            timestamps.append(timestamp)
    for key in ("created_at", "updated_at", "status_changed_at"):
        timestamp = value.get(key)
        if isinstance(timestamp, int):
            timestamps.append(timestamp)
    return ids, max(timestamps)


def new_audit_id(timestamp_ms: int, reserved_ids: set[str]) -> str:
    while True:
        candidate = f"ms_legacy_pause_{timestamp_ms}_{secrets.token_hex(16)}"
        if candidate not in reserved_ids:
            return candidate


def prepare_conversion(
    path: pathlib.Path,
    reason: str,
    audit_id: str | None = None,
    timestamp_ms: int | None = None,
) -> Conversion | None:
    original_bytes = path.read_bytes()
    original = original_bytes.decode("utf-8")
    value = load_yaml(original)
    if validate_already_converted(value):
        return None
    if not isinstance(value, dict) or value.get("status") != "parked":
        raise ValueError("expected legacy status parked or an audited prior conversion")
    if "pause_reason" in value:
        raise ValueError("legacy parked record must not contain pause_reason")
    legacy_reason = str(value.get("park_reason") or "").strip()
    pause_reason = legacy_reason or reason
    existing_ids, latest_timestamp = audit_metadata(value)
    timestamp_ms = timestamp_ms if timestamp_ms is not None else max(
        time.time_ns() // 1_000_000, latest_timestamp + 1
    )
    audit_id = audit_id or new_audit_id(timestamp_ms, existing_ids)
    if audit_id in existing_ids:
        raise ValueError(f"audit id already exists: {audit_id}")
    if timestamp_ms <= latest_timestamp:
        raise ValueError(
            f"audit timestamp {timestamp_ms} must be newer than existing {latest_timestamp}"
        )
    lines = original.splitlines(keepends=True)
    if lines and not lines[-1].endswith(("\n", "\r")):
        lines[-1] += "\n"
    replace_top_level(lines, "status", "paused")
    set_pause_reason(lines, pause_reason)
    replace_top_level(lines, "assigned_pane_id", "null")
    clear_lane_assignment(lines)
    close_active_attempts(lines, timestamp_ms)
    append_audit_milestone(lines, audit_id, pause_reason, reason, timestamp_ms)
    converted = "".join(lines)
    parsed = load_yaml(converted)
    if parsed.get("status") != "paused" or parsed.get("pause_reason") != pause_reason:
        raise ValueError("conversion validation failed")
    if parsed.get("assigned_pane_id") is not None:
        raise ValueError("active assignment was not cleared")
    if parsed.get("lane", {}).get("pane_id") is not None:
        raise ValueError("active lane assignment was not cleared")
    if any(attempt.get("closed_at") is None for attempt in parsed.get("execution_attempts", [])):
        raise ValueError("active execution attempt was not closed")
    return Conversion(path, original_bytes, converted, audit_id, timestamp_ms)


def load_candidate(path: pathlib.Path) -> tuple[object, set[str], int]:
    value = load_yaml(path.read_text(encoding="utf-8"))
    if validate_already_converted(value):
        return value, set(), 0
    if not isinstance(value, dict) or value.get("status") != "parked":
        raise ValueError("expected legacy status parked or an audited prior conversion")
    if "pause_reason" in value:
        raise ValueError("legacy parked record must not contain pause_reason")
    ids, timestamp = audit_metadata(value)
    return value, ids, timestamp
