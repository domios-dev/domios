"""WHAT: Render every required cell, including missing and pending evidence.
WHY: An incomplete matrix must never be presented as a passing release gate.
ORACLE: Only a transcript-backed, independently rechecked result becomes PASS.
"""

import argparse
from collections import Counter
import json
import os
from pathlib import Path
import sys

from fixtures import HERE, PROVIDERS
from oracles import judge
from scenarios import CASES
from sentinel_oracles import judge_sentinel, judge_sentinel_feed
from coordinator_oracles import judge_coordinator
from attribution import binding_ready
from launch_prompt import contract_blocker as launch_prompt_blocker
from launch_prompt import judge as judge_launch_prompt
from launch_prompt import matrix_cells


def cells():
    for provider in PROVIDERS:
        for case in CASES:
            for jev in ("off", "on"):
                yield provider, case, jev, "default", ""
    for variant in ("default", "busy", "fail"):
        for jev in ("off", "on"):
            yield "codex", "update-coordinator", jev, variant, ""
    for variant in ("git", "fake-ci", "regex-no-jev", "budget-exhausted"):
        for jev in ("off", "on"):
            yield "host", "sentinel", jev, variant, ""
    yield "host", "sentinel", "on", "activity-feed", ""
    decision = json.loads((HERE / "contracts.json").read_text())["decision"]
    for point in disagreement_points(decision):
        yield point["provider"], point["case"], "on", "disagreement", point["id"]


def launch_cells():
    for provider, folder, jev in matrix_cells():
        yield provider, "launch-prompt", jev, folder, ""


def disagreement_points(decision):
    points = {}
    for point in [*decision.get("required_disagreements", []), *decision.get("covered_points", [])]:
        points[(point["provider"], point["case"], point["id"])] = point
    return list(points.values())


def records(root):
    results = {}
    for path in sorted(root.glob("transcripts/*/*/result.json")):
        result = json.loads(path.read_text())
        if result.get("case") == "smoke":
            continue
        if result.get("case") == "launch-prompt":
            key = (result["provider"], result["case"], result["jev"], result.get("folder", ""), "")
        else:
            key = tuple(result[field] for field in ("provider", "case", "jev", "variant"))
            key += (result.get("decision_point", ""),)
        if result.get("status") == "PASS":
            transcript = path.parent / "transcript.jsonl"
            try:
                if not transcript.is_file() or not transcript.stat().st_size:
                    raise ValueError("Missing transcript")
                lines = [json.loads(line) for line in transcript.read_text().splitlines()]
                if any(line["kind"] == "finding" for line in lines):
                    raise ValueError("Transcript contains an unresolved finding")
                oracle = judge
                if result["case"] == "launch-prompt":
                    if Path(result["evidence"].get("transcript_path", "")).resolve() != transcript.resolve():
                        raise ValueError("Evidence transcript path differs from the retained cell transcript")
                    evidence = {**result["evidence"], "transcript": lines}
                    verdict = judge_launch_prompt(
                        evidence, json.loads((HERE / "contracts.json").read_text())["launch_prompt_matrix"])
                    if result.get("decided_by") != evidence.get("decided_by"):
                        raise ValueError("Result and oracle disagree on deciding arm")
                    if result.get("outcome") != verdict["outcome"] or evidence.get("outcome") != verdict["outcome"]:
                        raise ValueError("Result and oracle disagree on launch-prompt outcome")
                    oracle = None
                elif result["case"] == "sentinel":
                    oracle = judge_sentinel_feed if result["variant"] == "activity-feed" else judge_sentinel
                if result["case"] == "update-coordinator":
                    oracle = judge_coordinator
                if oracle:
                    oracle(result["evidence"])
                expected_path = result.get("decided_by") if result["case"] == "launch-prompt" else result["evidence"]["path"]
                if result.get("path") != expected_path:
                    raise ValueError("Result and oracle disagree on decision path")
            except Exception as error:
                result = {**result, "status": "FAIL", "reason": f"Report rejected PASS: {error}"}
        result["artifact"] = str(path.parent)
        old = results.get(key)
        if old is None or int(result["run_id"]) > int(old["run_id"]):
            results[key] = result
    return results


def escaped(value):
    return str(value).replace("|", "\\|").replace("\n", " ")


def render_launch_matrix(results, contract):
    lines = [
        "### Cron launch-prompt matrix", "",
        "Cron sets `provisioned_cwd` to the selected job cwd. Trust answers require that exact provenance; otherwise the oracle requires one owner notice and zero authorized keys.",
        "The existing trust-hidden worker cells in both Jev arms remain in the Provider prompt matrix above.", "",
        "| Provider | Folder | Jev | Expected outcome | Result | Deciding arm | Transcript / reason |",
        "|---|---|---|---|---|---|---|",
    ]
    counts = Counter()
    blocker = launch_prompt_blocker(contract)
    providers = contract.get("providers", {})
    for cell in launch_cells():
        provider, _, jev, folder, _ = cell
        result = results.get(cell, {})
        provider_contract = providers.get(provider, {})
        status = result.get("status", "NOT RUN")
        if not result and provider_contract.get("status") == "unsupported":
            status = "UNSUPPORTED"
        counts[status] += 1
        evidence = result.get("evidence", {})
        arm = result.get("decided_by") or evidence.get("decided_by") or "unobserved"
        artifact = result.get("artifact")
        reason = result.get("reason") or "; ".join(
            value for value in (blocker, provider_contract.get("reason")) if value
        ) or "No live receipt"
        if status == "UNSUPPORTED":
            reason = f"{reason}; evidence: {provider_contract.get('evidence')}"
        if artifact:
            transcript_path = os.path.relpath(Path(artifact) / "transcript.jsonl", HERE).replace(os.sep, "/")
            detail = f"[transcript]({transcript_path}): {escaped(reason)}"
        else:
            detail = escaped(reason)
        if evidence.get("prompt_kind") not in (None, "directory_trust"):
            expected = "non-trust prompt retained; one owner notice; zero keys"
        elif evidence.get("provenance_authorized"):
            expected = "provider-declared trust answer; cron run Success"
        else:
            expected = "answer only with matching provenance; otherwise one owner notice"
        lines.append("| " + " | ".join(map(escaped,
                     (provider, folder, jev, expected, status, arm, detail))) + " |")
    lines += ["", "Launch-prompt counts: " +
              "; ".join(f"{key}={value}" for key, value in sorted(counts.items())) + ".", ""]
    return lines, counts


def render(results):
    accepted = json.loads((HERE / "contracts.json").read_text())["sentinel"]["not_applicable"]
    lines = [
        "# Provider prompt matrix", "",
        "Release gate remains closed until every required cell passes or has explicit owner acceptance.",
        "Missing evidence is NOT RUN. Pending contracts are PENDING. Neither is a pass.", "",
        "Off is the heuristic arm. On exercises connected Jev primary and records any actual fallback.",
        "Deciding paths require bound occurrence/point/launch/session evidence and actual selected arms; Jev configuration is insufficient.", "",
        "| Provider | Case | Jev | Variant | Decision point | Result | Jev primary | Heuristic / regex | Deciding path | Transcript / finding |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    counts = Counter()
    for cell in cells():
        result = results.get(cell, {})
        exception = next((item for item in accepted if cell[:4] ==
                          ("host", "sentinel", item["jev"], item["variant"])), None)
        if exception:
            result = {"status": "N/A", "reason": exception["reason"] + " Authority: " + exception["authority"],
                      "path": "not_applicable_disconnected_regex"}
        elif result.get("status") == "N/A":
            result = {"status": "FAIL", "reason": "No owner acceptance exists for this N/A cell"}
        status = result.get("status", "NOT RUN")
        counts[status] += 1
        path = result.get("path") or "unobserved"
        evidence = result.get("evidence", {})
        primary = "recorded" if evidence.get("jev_arm") else "unobserved"
        heuristic = "recorded" if evidence.get("heuristic_arm") else "unobserved"
        if exception:
            primary, heuristic = "N/A: disconnected", "N/A: regex does not spend model budget"
        if cell[3] == "activity-feed":
            primary = heuristic = "N/A: visibility projection"
        artifact = result.get("artifact")
        note = result.get("reason", "No live gate executed")
        if artifact:
            note = f"[transcript]({artifact}/transcript.jsonl): {escaped(note)}"
        lines.append("| " + " | ".join(map(escaped, (*cell, status, primary, heuristic, path, note))) + " |")
    launch_lines, _ = render_launch_matrix(
        results, json.loads((HERE / "contracts.json").read_text())["launch_prompt_matrix"])
    lines += ["", *launch_lines,
              "Counts: " + "; ".join(f"{key}={value}" for key, value in sorted(counts.items())) + ".", "",
              "Auditor disagreement cells: pending the covered-point list in contracts.json.", "",
              "Red-green demonstration: deferred to canonical QA; no executed demonstration is claimed.", "",
              "Source facts, executed findings and hypotheses are separate in README.md.", ""]
    return "\n".join(lines), counts


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=HERE / "RESULTS.md")
    options = parser.parse_args()
    results = records(options.root)
    body, counts = render(results)
    options.out.write_text(body)
    print("; ".join(f"{key}={value}" for key, value in sorted(counts.items())))
    contracts = json.loads((HERE / "contracts.json").read_text())
    return exit_code(counts, contracts,
                     launch_matrix_complete(results, contracts["launch_prompt_matrix"]))


def launch_matrix_complete(results, contract):
    if launch_prompt_blocker(contract):
        return False
    for provider, folder, jev in matrix_cells():
        provider_contract = contract.get("providers", {}).get(provider, {})
        if provider_contract.get("status") == "unsupported":
            if not provider_contract.get("reason") or not provider_contract.get("evidence"):
                return False
            continue
        key = (provider, "launch-prompt", jev, folder, "")
        result = results.get(key, {})
        artifact = result.get("artifact")
        if (result.get("status") != "PASS" or not result.get("decided_by") or
                not artifact or not (Path(artifact) / "transcript.jsonl").is_file()):
            return False
    return True


def exit_code(counts, contracts, launch_complete=False):
    points = disagreement_points(contracts["decision"])
    complete = (counts and counts.get("PASS", 0) >= 86 + len(points) and counts.get("N/A", 0) == 1 and
                counts.get("PASS", 0) + counts.get("N/A", 0) == sum(counts.values()))
    bound = (binding_ready(contracts["decision"]) and
             contracts["decision"].get("covered_points_status") == "bound" and
             bool(contracts["decision"].get("covered_points")) and
             all(point.get("status") == "bound" for point in points) and
             all(contracts[key]["status"] == "bound" for key in
                 ("coordinator", "sentinel", "isolation", "update_fixture")))
    return 0 if complete and bound and launch_complete else 1


if __name__ == "__main__":
    sys.exit(main())
