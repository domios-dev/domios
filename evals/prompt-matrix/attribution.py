"""WHAT: Reject classification evidence without exact occurrence correlation.
WHY: Two same-tab journal records can describe different launches or prompts.
ORACLE: Distinct completed ids bind both arms and the selected verdict to one occurrence.
"""

from transport import Finding, Pending


IDENTITY = ("tab", "launch", "session", "occurrence", "point")
REQUIRED = (*IDENTITY, "observation_id", "completed", "decided_by",
            "heuristic_verdict", "adapter_result", "selected_verdict")
PATH_MAPPING = {"adapter": "adapter_verdict", "heuristic": "heuristic",
                **{f"fallback({reason})": f"fallback_{reason}" for reason in
                   ("no_adapter", "timeout", "other", "low_confidence", "error", "budget")}}


def binding_ready(contract):
    fields = contract.get("fields", {})
    return (contract.get("status") == "bound" and
            all(isinstance(fields.get(key), str) and fields[key] for key in REQUIRED) and
            len(set(fields.values())) == len(fields))


def observations(rows, expected, contract, jev, minimum=2, disagreement=False):
    if not binding_ready(contract):
        raise Pending("Auditor occurrence/point/completion/paired-arm schema is not bound")
    fields = contract["fields"]
    if any(expected.get(key) in (None, "") for key in IDENTITY):
        raise Finding("Expected decision identity lacks launch/session/occurrence/point")
    relevant = [row for row in rows if row.get(fields["tab"]) == expected["tab"] and
                fields["decided_by"] in row]
    completed = []
    ids = set()
    for row in relevant:
        if any(row.get(fields[key]) != expected[key] for key in IDENTITY):
            raise Finding("Decision belongs to wrong point/session/launch/occurrence")
        if row.get(fields["completed"]) is not True:
            raise Finding("Decision observation is not completed")
        identifier = row.get(fields["observation_id"])
        if isinstance(identifier, bool) or not isinstance(identifier, int) or identifier <= 0:
            raise Finding("Observation id is missing or stale")
        if identifier in ids or (completed and identifier <= completed[-1]["id"]):
            raise Finding("Duplicate or stale observation id")
        ids.add(identifier)
        source = row.get(fields["decided_by"])
        if source not in PATH_MAPPING:
            raise Finding("Unknown deciding arm")
        heuristic = row.get(fields["heuristic_verdict"])
        adapter = row.get(fields["adapter_result"])
        selected = row.get(fields["selected_verdict"])
        if heuristic in (None, "") or selected in (None, ""):
            raise Finding("Paired heuristic or selected verdict is missing")
        if jev == "off" and source not in ("heuristic", "fallback(no_adapter)"):
            raise Finding("Disconnected cell names the wrong deciding arm")
        if source == "adapter":
            if not isinstance(adapter, dict) or adapter.get("status") != "verdict":
                raise Finding("Adapter deciding arm has no recorded verdict")
            if adapter.get("verdict") != selected:
                raise Finding("Selected verdict differs from deciding adapter arm")
        elif selected != heuristic:
            raise Finding("Selected verdict differs from deciding heuristic arm")
        if jev == "on":
            if not isinstance(adapter, dict) or adapter.get("status") not in ("verdict", "fallback"):
                raise Finding("Connected cell has no explicit adapter result")
            if adapter["status"] == "fallback" and source != f"fallback({adapter.get('reason')})":
                raise Finding("Adapter fallback reason differs from deciding arm")
            if source == "heuristic":
                raise Finding("Connected cell does not attribute its fallback reason")
        if disagreement and (not isinstance(adapter, dict) or adapter.get("status") != "verdict" or
                             adapter.get("verdict") in (None, "", heuristic)):
            raise Finding("Same-observation adapter/heuristic disagreement was not proven")
        completed.append({"id": identifier, "path": PATH_MAPPING[source], "row": row})
    if len(completed) < minimum:
        raise Finding("Two distinct completed observations of the same occurrence are required")
    return completed


def audit(evidence, contract, minimum=2):
    completed = observations(evidence.get("decision_rows", []), evidence.get("decision_identity", {}),
                             contract, evidence.get("jev"), minimum,
                             evidence.get("variant") == "disagreement")
    if evidence.get("path") != completed[-1]["path"]:
        raise Finding("Claimed path differs from the correlated deciding arm")
    return completed
