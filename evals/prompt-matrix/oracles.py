"""WHAT: Judge captured prompt-policy evidence independently of provokers.
WHY: Setup success and disappearing prompts must not become false greens.
ORACLE: Observed classification precedes a policy decision and its far end.
"""

import json
from pathlib import Path

from transport import Finding, objects
from attribution import PATH_MAPPING, audit, observations


PATHS = set(PATH_MAPPING.values())


def require(value, message):
    if not value:
        raise Finding(message)


def ready(tab):
    return (tab.get("boot_stage") == "agent_ready" and
            bool(tab.get("agent_session_id")) and
            bool(tab.get("session_binding_nonce")) and
            not tab.get("pending_prompt"))


def prompt_kind(tab):
    state = tab.get("agent_state") or {}
    verdict = state.get("classification", state)
    return verdict.get("subtype") or tab.get("pending_prompt_kind")


def tab_notifications(payload, tab, baseline):
    found = {}
    for item in objects(payload):
        identifier = item.get("id")
        if not identifier or identifier in baseline:
            continue
        provenance = item.get("provenance") or {}
        owner = item.get("tab_id") or provenance.get("tab_id")
        if owner == tab and item.get("level") != "verbose":
            found[identifier] = item
    return list(found.values())


def decision_path(journal, tab, jev, expected=None, contract=None):
    if not expected or not contract or expected.get("tab") != tab:
        return None
    completed = observations(journal, expected, contract, jev, minimum=1)
    return completed[-1]["path"]


def judge(evidence):
    contract = json.loads((Path(__file__).parent / "contracts.json").read_text())["decision"]
    audit(evidence, contract)
    return judge_outcome(evidence)


def judge_outcome(evidence):
    case = evidence["case"]
    require(evidence.get("instance_proven"), "Named instance identity was not proven")
    require(evidence.get("real_provider"), "Real provider launch was not observed")
    require(evidence.get("prompt_observed"), "Required real prompt/state was never observed")
    require(evidence.get("path") in PATHS, "Deciding path has no journal evidence")
    require(evidence.get("heuristic_arm"), "Heuristic arm has no recorded verdict")
    if evidence.get("jev") == "on":
        require(evidence.get("jev_arm"), "Jev primary arm has no recorded verdict or fallback")
    if evidence.get("variant") == "disagreement":
        require(evidence.get("disagreement"), "Required heuristic/Jev disagreement was not observed")
    require(evidence.get("precondition_n", 0) < evidence.get("outcome_n", 0),
            "Outcome did not follow the provoking precondition")
    if case == "permission":
        require(evidence.get("pending_same"), "Permission request was cleared or changed")
        require(evidence.get("answers") == 0, "Permission prompt was answered")
        require(evidence.get("tool_completed") is False, "Permission-requiring tool completed")
        require(evidence.get("observation_barrier"), "Permission check has no completed policy observation")
    if evidence.get("expected") == "answer":
        require(evidence.get("answer_committed"), "Needed automatic answer was not committed")
        require(evidence.get("far_end"), "Answer did not reach its provider-visible far end")
    elif evidence.get("expected") == "notify":
        require(evidence.get("notifications") == 1, "Expected exactly one owner notification")
        require(evidence.get("route_proven"), "Notification did not reach the intended owner surface")
        require(evidence.get("dedup_barrier"), "Notification dedup/coalescing was not exercised")
    else:
        raise Finding("Case has no bound policy oracle")
    if case == "crash":
        require(evidence.get("own_pid_signalled"), "Crash did not target the tab's exact provider PID")
        require(evidence.get("restart_count", 0) <= evidence.get("restart_limit", -1),
                "Automatic restart bound exceeded or was not captured")
    return "PASS"


def load_journal(data_dir):
    rows = []
    for name in ("journal.previous.jsonl", "journal.jsonl"):
        path = Path(data_dir) / "supporter" / name
        if path.exists():
            require(path.stat().st_size <= 2 * 1024 * 1024, "Supporter journal exceeds read cap")
            rows += [json.loads(line) for line in path.read_text().splitlines() if line]
    return rows
