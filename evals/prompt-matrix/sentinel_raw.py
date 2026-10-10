"""WHAT: Validate the reviewed persisted Sentinel provenance contract.
WHY: A selected attempt or completed routing API cannot prove a model call or recipient.
ORACLE: Captured scope/hash/UUID and classification survive every ordered route stage.
"""

import uuid

from oracles import require


SCHEMA = {"time", "kind", "sentinel_id", "tab", "team_id", "workspace_id", "state_hash",
          "evaluation_id", "observation_id", "classification_source", "classification_reason",
          "label", "confidence", "stage", "route_status", "action", "reason", "route",
          "route_target", "recipient_receipt", "asked", "budget"}
SCOPE = ("sentinel_id", "tab", "team_id", "workspace_id")
CAPTURED = (*SCOPE, "state_hash", "evaluation_id", "observation_id", "classification_source",
            "classification_reason", "label", "confidence", "route", "route_target")
STATUSES = {"decision": "not_attempted", "route_claim": "claimed", "route_completed": "api_completed",
            "route_failed": "failed", "skip": "not_attempted"}


def uuid_value(value):
    if not isinstance(value, str):
        return False
    try:
        return str(uuid.UUID(value)) == value
    except ValueError:
        return False


def validate_raw(rows, scope):
    require(all(key in scope for key in SCOPE), "Sentinel captured scope is incomplete")
    groups = {}
    for row in rows:
        require(SCHEMA <= row.keys() and row["kind"] == "sentinel", "Sentinel raw journal schema is incomplete")
        require(all(row[key] == scope[key] for key in SCOPE), "Sentinel raw journal has foreign captured scope")
        evaluation = row["evaluation_id"]
        require(uuid_value(evaluation), "Sentinel evaluation id is not a UUID")
        source, observation = row["classification_source"], row["observation_id"]
        require(source in ("adapter", "regex", "disconnected", "not_attempted"), "Unknown Sentinel classification source")
        require(observation == evaluation if source in ("adapter", "regex") else observation is None,
                "Sentinel observation id does not name the selected attempt")
        stage = row["stage"]
        require(stage in STATUSES and row["route_status"] == STATUSES[stage], "Sentinel stage/route status mismatch")
        require(row["recipient_receipt"] is None, "Raw Sentinel journal invented a recipient receipt")
        if row["classification_reason"] == "unchanged":
            require(source == "not_attempted" and observation is None and row["label"] is None and
                    row["route"] is None and row["route_target"] is None and stage == "decision",
                    "Unchanged evaluation cannot be a new classification or route")
        if row["reason"].startswith("probe_"):
            require(source == "not_attempted" and observation is None and row["state_hash"] is None and
                    row["label"] is None and row["route"] is None and stage == "skip",
                    "Failed probe retained stale classification/hash")
        if stage.startswith("route_"):
            require(row["classification_reason"] == "transition" and row["route"] in ("notify", "send", "cron", "stop") and
                    bool(row["label"]) and row["state_hash"] is not None,
                    "Sentinel route stage has no captured confident transition")
        if stage == "route_failed":
            require(row["reason"] == "route_failed" and row["action"] == "skip",
                    "Failed route lost classification provenance or failure result")
        elif stage == "route_completed":
            require(row["action"] == row["route"], "Completed routing API has the wrong action")
        else:
            require(row["action"] == stage, "Sentinel stage/action mismatch")
        group = groups.setdefault(evaluation, [])
        if group:
            require(all(row[key] == group[0][key] for key in CAPTURED),
                    "Sentinel route stages changed captured scope/hash/UUID/classification")
        group.append(row)
    for group in groups.values():
        stages = [row["stage"] for row in group]
        require(len(stages) == len(set(stages)), "Duplicate Sentinel stage in one evaluation")
        require(stages in (["decision"], ["skip"], ["decision", "route_claim"],
                           ["decision", "route_claim", "route_completed"],
                           ["decision", "route_claim", "route_failed"]),
                "Sentinel route stages are out of order")
    return groups
