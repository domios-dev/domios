"""WHAT: Independently judge Courier run ordering and recipient receipts.
WHY: Durable restart claims and installer success do not prove a resumed tab.
ORACLE: One run completes before eligible restarts; own-session version receipts prove the far end.
"""

import json
from pathlib import Path

from attribution import audit
from oracles import require


def judge_coordinator_outcome(evidence):
    require(evidence.get("instance_proven"), "Coordinator named instance was not proven")
    require(evidence.get("real_prompt"), "Coordinator real update prompt was not observed")
    require(evidence.get("isolated_update_scope"), "Coordinator updater can escape its isolated prefix")
    provider, target = evidence["provider"], evidence["target_version"]
    rows = evidence["journal"]
    require(all(row.get("provider") == provider and row.get("target_version") ==
                (None if row.get("label") == "safe_choice_answered" else target)
                for row in rows), "Coordinator includes a foreign provider or target")
    starts = [index for index, row in enumerate(rows) if row.get("label") == "update_started"]
    require(len(starts) == 1, "Expected exactly one provider update run")
    variant = evidence["variant"]
    label = "update_failed" if variant == "fail" else "update_succeeded"
    finishes = [index for index, row in enumerate(rows) if row.get("label") in
                ("update_succeeded", "update_failed")]
    require(len(finishes) == 1 and rows[finishes[0]]["label"] == label and starts[0] < finishes[0],
            "Coordinator completion is missing, duplicated or out of order")
    notices = evidence.get("owner_notices", [])
    summary = evidence.get("persisted_run", {}).get("summary_id")
    require(len(notices) == 1 and bool(summary) and notices[0].get("id") == summary and
            notices[0].get("level") == ("failure" if variant == "fail" else "verbose") and
            notices[0].get("push_failed") is not True,
            "Expected exactly one observable owner summary matching the persisted notice")
    tabs = evidence["tabs"]
    prompt_tabs = evidence["prompt_tabs"]
    safe = [row for row in rows if row.get("label") == "safe_choice_answered"]
    require(sorted(row.get("tab") for row in safe) == sorted(prompt_tabs),
            "Each prompt tab must have exactly one provider-declared safe choice")
    receipts = evidence.get("safe_receipts", [])
    require(sorted(row.get("tab") for row in receipts) == sorted(prompt_tabs),
            "Safe-choice claims lack provider-side prompt disappearance receipts")
    for receipt in receipts:
        require(receipt.get("occurrence") == evidence["prompt_occurrences"].get(receipt["tab"]) and
                receipt.get("provider_ready") is True and receipt.get("choice") == evidence["safe_choice"],
                "Safe choice reached the wrong prompt occurrence or provider state")
    restarts = [(index, row) for index, row in enumerate(rows)
                if row.get("label") in ("restart_requested", "resumed", "fresh_launch", "restart_failed")]
    launches = evidence.get("restart_launches", [])
    versions = evidence.get("version_receipts", [])
    if variant == "fail":
        require(not restarts and not launches and not versions,
                "Failed update restarted a tab or claimed a resumed version")
        require(evidence.get("failure_barrier"), "Failure has no completed post-failure observation barrier")
        return "OUTCOME VERIFIED"
    require(sorted(row.get("tab") for _, row in restarts if row["label"] == "restart_requested") == sorted(tabs),
            "Each eligible tab needs exactly one restart request")
    require(sorted(row.get("tab") for _, row in restarts if row["label"] == "resumed") == sorted(tabs),
            "Every coordinator tab must resume its own session exactly once")
    require(not any(row["label"] in ("fresh_launch", "restart_failed") for _, row in restarts),
            "Coordinator fresh launch or failure cannot replace required resumption")
    require(sorted(row.get("tab") for row in launches) == sorted(tabs) and
            sorted(row.get("tab") for row in versions) == sorted(tabs),
            "Restart claims lack one independent launch/version receipt per tab")
    for tab in tabs:
        request = next(index for index, row in restarts if row["tab"] == tab and row["label"] == "restart_requested")
        resumed = next(index for index, row in restarts if row["tab"] == tab and row["label"] == "resumed")
        require(finishes[0] < request < resumed, "Restart did not follow successful installation")
        receipt = next(row for row in versions if row["tab"] == tab)
        launch = next(row for row in launches if row["tab"] == tab)
        require(receipt.get("session") == evidence["sessions"][tab] == launch.get("session") and
                receipt.get("launch") == launch.get("launch") and
                receipt.get("version") == target and receipt.get("source") == "own_session_tool_result" and
                receipt.get("exit_code") == 0,
                "Version receipt is stale, foreign, global or not the target version")
    if variant == "busy":
        busy = evidence["busy_tab"]
        checkpoint = evidence.get("busy_checkpoint", {})
        require(checkpoint.get("install_completed") is True and checkpoint.get("active_turn") is True and
                checkpoint.get("tab") == busy and checkpoint.get("restart_count") == 0,
                "Busy checkpoint does not prove installation finished while the tab remained active")
        idle = evidence.get("idle_receipt", {})
        require(idle.get("tab") == busy and idle.get("session") == evidence["sessions"][busy] and
                idle.get("active_turn") is False and idle.get("active_attempt") is False,
                "Busy tab has no own-session eligible idle receipt")
        require(evidence.get("install_n", 0) < idle.get("n", 0) < evidence.get("busy_restart_n", 0),
                "Busy restart did not follow installation and eligible idle")
    return "OUTCOME VERIFIED"


def judge_coordinator(evidence):
    contract = json.loads((Path(__file__).parent / "contracts.json").read_text())["decision"]
    audit(evidence, contract)
    judge_coordinator_outcome(evidence)
    return "PASS"
