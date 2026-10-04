"""WHAT: Judge cron launch prompts across cwd folders, providers and Jev arms.
WHY: A hidden launch must answer only its provider-declared trust prompt under provenance.
ORACLE: Transcript prompt, producer journal, owner notice and cron history agree for one exact cell.
"""

import os

from fixtures import PROVIDERS
from transport import Finding, Pending


FOLDERS = ("owner-dir", "new-dir", "lane", "home")
JEV_ARMS = ("off", "on")
TRUST_KIND = "directory_trust"
AUTOMATIC_PHASES = {"action_authorized", "action_stale"}
REQUIRED_BINDINGS = (
    "prompt", "provisioned_cwd", "decided_by", "observation_id",
    "action_phase", "provider_answer", "owner_notice", "cron_status", "non_trust_prompt",
    "provider_session", "provider_auth_policy",
)


def matrix_cells(providers=PROVIDERS):
    for provider in providers:
        for folder in FOLDERS:
            for jev in JEV_ARMS:
                yield provider, folder, jev


def contract_blocker(contract):
    bindings = contract.get("bindings", {})
    for field in REQUIRED_BINDINGS:
        binding = bindings.get(field, {})
        if binding.get("status") != "bound" or not binding.get("source") or not binding.get("field"):
            source = binding.get("source", "source path:symbol not recorded")
            return f"Unbound launch-prompt field {field}: {source}"
    if contract.get("status") != "bound":
        return contract.get("reason", "Launch-prompt contract is not bound")
    owner = contract.get("owner_decision", {})
    if (owner.get("status") != "recorded" or owner.get("choice") not in ("A", "B", "C") or
            not owner.get("stride") or not owner.get("rule")):
        return "Owner isolation choice, explicit rule and stride are not recorded"
    return None


def _require(value, message):
    if not value:
        raise Finding(message)


def _prompt_capture(evidence):
    transcript = evidence.get("transcript")
    _require(isinstance(transcript, list) and transcript,
             "Missing transcript records")
    captures = [record.get("value", {}) for record in transcript
                if record.get("kind") == "launch_prompt_capture"]
    _require(len(captures) == 1, "Transcript must contain exactly one real prompt capture")
    capture = captures[0]
    _require(capture.get("tab_id") == evidence.get("tab_id"),
             "Prompt transcript belongs to another tab")
    _require(capture.get("provider") == evidence.get("provider"),
             "Prompt transcript belongs to another provider")
    _require(isinstance(capture.get("kind"), str) and capture["kind"],
             "Transcript lacks the provider prompt classification")
    prompt = capture.get("prompt")
    options = prompt.get("options") if isinstance(prompt, dict) else None
    _require(isinstance(prompt, dict) and prompt.get("question") and
             isinstance(options, list) and options,
             "Transcript lacks the real structured provider prompt")
    _require(isinstance(capture.get("screen"), str) and capture["screen"],
             "Transcript lacks the provider screen that displayed the prompt")
    values = [item.get("value") for item in options if isinstance(item, dict)]
    _require(len(values) == len(options) and all(isinstance(value, str) and value for value in values),
             "Provider prompt options are incomplete")
    return capture


def _rows(evidence):
    rows = evidence.get("journal_rows")
    _require(isinstance(rows, list), "Producer journal rows are absent")
    tab = evidence.get("tab_id")
    exact = [row for row in rows if row.get("tab") == tab or row.get("tab_id") == tab]
    _require(exact, "Producer journal has no rows for the exact cron tab")
    return exact


def _observation_ids(rows):
    values = []
    for row in rows:
        identifier = row.get("observation_id")
        if isinstance(identifier, int) and not isinstance(identifier, bool) and identifier > 0:
            values.append(identifier)
    return values


def same_path(left, right):
    if not left or not right:
        return False
    return os.path.normcase(os.path.realpath(left)) == os.path.normcase(os.path.realpath(right))


def judge(evidence, contract):
    blocker = contract_blocker(contract)
    if blocker:
        raise Pending(blocker)

    provider, folder, jev = (evidence.get("provider"), evidence.get("folder"), evidence.get("jev"))
    _require((provider, folder, jev) in set(matrix_cells()), "Unknown launch-prompt matrix cell")
    _require(evidence.get("transcript_path"), "Missing transcript path")
    _require(same_path(evidence.get("cwd"), evidence.get("expected_cwd")),
             "Provider launch cwd differs from the selected folder")
    provenance_authorized = bool(evidence.get("provisioned_cwd") and
                                 same_path(evidence.get("cwd"), evidence.get("provisioned_cwd")))
    _require(evidence.get("provenance_authorized") is provenance_authorized,
             "Recorded provenance predicate differs from observed cwd fields")
    capture = _prompt_capture(evidence)
    rows = _rows(evidence)

    prompt_kind = capture["kind"]
    action_rows = [row for row in rows if row.get("phase") in AUTOMATIC_PHASES]
    route_rows = [row for row in rows if row.get("action") == "unattended_launch_prompt"]
    _require(route_rows, "Unattended prompt decision is absent from the producer journal")
    route_ids = _observation_ids(route_rows)
    decided_by = route_rows[-1].get("decided_by")
    if jev == "on":
        _require(all(row.get("decided_by") == "adapter" for row in route_rows),
                 "Connected Jev cell has the wrong deciding arm")
    else:
        _require(all(row.get("decided_by") in ("heuristic", "fallback(no_adapter)")
                     for row in route_rows),
                 "Disconnected Jev cell has the wrong deciding arm")

    _require(not any(row.get("phase") == "action_stale" for row in rows),
             "Producer journal records a stale automatic action")
    deterministic = [row for row in rows if row.get("reason") == "deterministic_directory_trust"]
    if prompt_kind != TRUST_KIND:
        _require(not action_rows and not deterministic,
                 "A key was authorized for a non-trust prompt")
        _require(evidence.get("owner_notices") == 1,
                 "Non-trust launch prompt must produce exactly one owner notice")
        _require(evidence.get("prompt_cleared") is False,
                 "Non-trust launch prompt was answered")
        run = evidence.get("cron_run", {})
        _require(evidence.get("status") == "AwaitingOwnerPrompt" and
                 run.get("status") == "AwaitingOwnerPrompt" and
                 run.get("run_id") == evidence.get("run_id"),
                 "Non-trust prompt lacks its retained owner-pending cron run")
        return {"outcome": "owner_notice", "decided_by": decided_by}

    _require(evidence.get("cwd"), "Cron launch cwd is absent")
    if action_rows or deterministic:
        _require(provenance_authorized,
                 "Unauthorized JEV cell answered a trust prompt without launch provenance")
        _require(prompt_kind == TRUST_KIND, "Automatic answer targeted a non-trust prompt")
        _require(len(route_ids) == len(route_rows) and len(set(route_ids)) >= 2 and
                 all(left < right for left, right in zip(route_ids, route_ids[1:])),
                 "Automatic action lacks two distinct observation ids")
        _require(len(action_rows) + len(deterministic) == 1,
                 "Automatic trust action was authorized more than once")
        _require(evidence.get("prompt_cleared") is True and
                 evidence.get("session_authenticated") is True and
                 evidence.get("role_marker") is True,
                 "Authorized trust answer did not reach the provider far end")
        _require(evidence.get("owner_notices") == 0,
                 "Answered trust prompt also produced an owner notice")
        run = evidence.get("cron_run", {})
        _require(evidence.get("status") == "finished", "Cron run did not reach its finished state")
        _require(run.get("status") == "Success" and run.get("finished_at") is not None and
                 run.get("run_id") == evidence.get("run_id") and run.get("session_id") and
                 run.get("session_id") == evidence.get("provider_session_id"),
                 "Answered cron run lacks its exact successful completion receipt")
        if jev == "on":
            _require(len(action_rows) == 1 and action_rows[0].get("phase") == "action_authorized",
                     "Adapter answer lacks one action_authorized producer row")
            _require(action_rows[0].get("observation_id") in route_ids,
                     "Authorized action is not correlated to an observed decision")
            _require(action_rows[0].get("reason") == "provider_safe_step_authorized" and
                     any(row.get("reason") == "declared_choice" for row in route_rows),
                     "Adapter action lacks the provider-declared safe-choice receipt")
        else:
            _require(len(deterministic) == 1,
                     "Heuristic answer lacks one deterministic trust action receipt")
        return {"outcome": "answer", "decided_by": decided_by}

    _require(not provenance_authorized,
             "Authorized cron provenance produced no provider-declared answer")
    _require(evidence.get("owner_notices") == 1,
             "Unauthorized launch prompt must produce exactly one owner notice")
    _require(evidence.get("prompt_cleared") is False,
             "Unauthorized trust prompt was answered")
    _require(not action_rows and not deterministic,
             "Unauthorized launch prompt sent a key")
    run = evidence.get("cron_run", {})
    _require(evidence.get("status") == "AwaitingOwnerPrompt" and
             run.get("status") == "AwaitingOwnerPrompt" and
             run.get("run_id") == evidence.get("run_id"),
             "Unauthorized trust prompt lacks its retained owner-pending cron run")
    return {"outcome": "owner_notice", "decided_by": decided_by}
