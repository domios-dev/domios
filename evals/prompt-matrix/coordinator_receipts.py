"""Independent native version tool receipts and ordered event phase controls."""

import json
import re

from oracles import require
from transport import Pending


def at(value, path):
    if not isinstance(path, str) or not path.startswith("/"):
        raise Pending("Reviewed producer receipt selector is absent")
    for token in path[1:].split("/"):
        key = token.replace("~1", "/").replace("~0", "~")
        if isinstance(value, list) and key.isdecimal() and int(key) < len(value):
            value = value[int(key)]
        elif isinstance(value, dict) and key in value:
            value = value[key]
        else:
            raise Pending(f"Actual producer receipt is absent at {path}")
    return value


def version_receipt(messages, expected, executable, nonce):
    candidates = []
    for result_row in messages:
        source_id, tool_id = result_row.get("toolCallSourceId"), result_row.get("toolId")
        if not source_id or not tool_id or result_row.get("toolPairing") != "exact":
            continue
        calls = [row for row in messages if row.get("toolCallSourceId") == source_id and
                 row.get("toolId") == tool_id and row.get("toolName") and row.get("toolInput")]
        if len(calls) != 1:
            continue
        call = calls[0]
        args = call.get("toolInput")
        if isinstance(args, str):
            try:
                args = json.loads(args)
            except ValueError:
                continue
        if not isinstance(args, dict) or args.get("command") != executable:
            continue
        result = result_row.get("toolResult")
        if result_row.get("toolExitCode") != 0 or result_row.get("toolError") or result_row.get("toolInterrupted"):
            continue
        if not isinstance(result, str):
            continue
        try:
            receipt = json.loads(result)
        except ValueError:
            continue
        if not isinstance(receipt, dict) or receipt.get("nonce") != nonce or receipt.get("argv") != ["codex", "--version"] or receipt.get("exit_code") != 0:
            continue
        values = re.findall(r"(?<![\w.])\d+\.\d+\.\d+(?![\w.])", receipt.get("stdout", ""))
        if values == [expected["version"]]:
            candidates.append(result_row)
    require(len(candidates) == 1, "Version needs one exact own-session tool call/result receipt")
    row = candidates[0]
    return {**expected, "source": "own_session_tool_result", "exit_code": row["toolExitCode"],
            "tool_call_source_id": row["toolCallSourceId"], "tool_id": row["toolId"], "nonce": nonce}


class Phases:
    def __init__(self, variant):
        require(variant in ("default", "busy", "fail"), "Unknown coordinator phase variant")
        self.variant = variant
        self.current = "preflight"
        self.sequence = 0
        self.released = False

    def advance(self, phase, sequence):
        paths = {
            "default": ("preflight", "prompt", "installed", "resumed", "verified"),
            "busy": ("preflight", "busy_started", "prompt", "installed_busy", "released", "idle", "resumed", "verified"),
            "fail": ("preflight", "prompt", "failed", "tree_stopped", "failure_barrier", "verified"),
        }
        path = paths[self.variant]
        require(path.index(self.current) + 1 < len(path) and
                path[path.index(self.current) + 1] == phase and sequence > self.sequence,
                "Coordinator phase is stale, duplicated or out of order")
        if phase == "released":
            require(not self.released, "Busy fixture was released twice")
            self.released = True
        self.current, self.sequence = phase, sequence
