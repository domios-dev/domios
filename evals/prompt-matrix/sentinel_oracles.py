"""WHAT: Verify Sentinel transitions, receipts, silence and budget refusal.
WHY: A route claim is weaker evidence than delivery to the intended recipient.
ORACLE: Every expected transition has one decision, claim, action and receipt.
"""

import hashlib
import json
from pathlib import Path

from oracles import PATHS, require
from sentinel_raw import validate_raw
from transport import Pending


def bucket_label(bucket):
    return hashlib.sha256(bucket.encode()).hexdigest()


def judge_sentinel_outcome(evidence):
    require(evidence.get("instance_proven"), "Named instance identity was not proven")
    require(evidence.get("probe_barrier"), "Unchanged probe completion was not observed")
    rows = evidence["journal"]
    require(all(row.get("sentinel_id") == evidence["sentinel_id"] and
                row.get("tab") == evidence["creator"] for row in rows),
            "Sentinel journal includes a foreign creator or watch")
    claims = [row for row in rows if row.get("action") == "route_claim"]
    completed = [row for row in rows if row.get("action") in ("notify", "send", "cron", "stop")]
    if evidence["variant"] == "budget-exhausted":
        require(evidence.get("budget_reached") is True, "Budget exhaustion was not proven")
        require(not claims and not completed and evidence["receipts"] == [],
                "Exhausted Sentinel routed an action")
        require(any(row.get("action") == "decision" and row.get("reason") == "budget_exhausted"
                    for row in rows), "Exhausted Sentinel has no refusal journal")
        require(evidence.get("deterministic_preserved"), "Budget fixture broke deterministic routing")
    else:
        expected = evidence["expected_buckets"]
        require(len(claims) == len(expected) and len(completed) == len(expected),
                "Expected exactly one claim and completed action per transition")
        require(evidence["receipts"] == expected, "Recipient has missing, duplicate or out-of-order receipts")
        for bucket, claim, action in zip(expected, claims, completed):
            label = bucket_label(bucket)
            require(claim.get("label") == label and action.get("label") == label and
                    action.get("action") == evidence["route_action"], "Route belongs to the wrong bucket/action")
            require(claim.get("state_hash") == action.get("state_hash"), "Claim and completion hash differ")
            require(sum(row.get("action") == "decision" and row.get("label") == label and
                        row.get("state_hash") == claim.get("state_hash") for row in rows) == 1,
                    "Transition lacks one matching classification")
            decision = next(row for row in rows if row.get("action") == "decision" and
                            row.get("state_hash") == claim.get("state_hash") and row.get("label") == label)
            require(rows.index(decision) < rows.index(claim) < rows.index(action),
                    "Route did not follow classification and claim")
    validate_raw(rows, evidence.get("raw_scope", {}))
    if evidence.get("regex_mode"):
        require(evidence.get("model_calls") == 0, "Regex mode called a model")
    elif evidence.get("jev") == "on":
        expected_calls = 0 if evidence["variant"] == "budget-exhausted" else len(evidence["expected_buckets"])
        expected_calls += int(evidence["variant"] == "git")
        require(evidence.get("model_calls") == expected_calls,
                "Changed/unchanged observations do not match exact model request count")
    require(evidence.get("unchanged_actions") == 0, "Unchanged output produced a new route")
    return "OUTCOME VERIFIED"


def judge_sentinel(evidence):
    judge_sentinel_outcome(evidence)
    require(evidence.get("path") in PATHS, "Sentinel deciding path has no journal evidence")
    contract = json.loads((Path(__file__).parent / "contracts.json").read_text())["sentinel"]
    if contract.get("attribution_status") != "bound":
        raise Pending("Sentinel same-event paired criteria remain pending owner disposition")
    require(evidence.get("heuristic_arm"), "Sentinel heuristic arm has no recorded verdict")
    if evidence.get("jev") == "on":
        require(evidence.get("jev_arm"), "Sentinel Jev arm has no verdict or explicit fallback")
    require(evidence.get("observation_ids"), "Sentinel decision has no observation identity")
    return "PASS"


def judge_sentinel_feed(evidence):
    require(evidence.get("instance_proven"), "Named instance identity was not proven")
    require(evidence.get("persisted_decision"), "Sentinel feed test lacks a completed persisted verdict")
    rows = evidence.get("new_entries", [])
    require(any(row.get("kind") == "sentinel" for row in rows),
            "Persisted Sentinel verdict is absent from the activity feed")
    for row in rows:
        require(all(key in row for key in ("time", "team_id", "kind", "label", "confidence_band",
                                           "action", "reason", "budget_reached", "fallback")),
                "Activity feed lacks a bound projection field")
        if row.get("kind") == "sentinel":
            require(not any(key in row for key in ("evaluation_id", "observation_id", "workspace_id",
                                                    "classification_source", "recipient_receipt")),
                    "Activity feed cannot substitute raw Sentinel UUID/provenance")
        require(not any(key in row for key in ("tab", "sentinel_id", "state_hash", "probe_output")),
                "Activity feed leaked raw Sentinel identity or probe data")
    return "PASS"
