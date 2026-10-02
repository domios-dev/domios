"""WHAT: Render every required cell, including missing and pending evidence.
WHY: An incomplete matrix must never be presented as a passing release gate.
ORACLE: Only a transcript-backed, independently rechecked result becomes PASS.
"""

import argparse
from collections import Counter
import json
from pathlib import Path
import sys

from fixtures import HERE, PROVIDERS
from oracles import judge
from scenarios import CASES
from sentinel_oracles import judge_sentinel, judge_sentinel_feed
from coordinator_oracles import judge_coordinator
from attribution import binding_ready


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
                if result["case"] == "sentinel":
                    oracle = judge_sentinel_feed if result["variant"] == "activity-feed" else judge_sentinel
                if result["case"] == "update-coordinator":
                    oracle = judge_coordinator
                oracle(result["evidence"])
                if result.get("path") != result["evidence"]["path"]:
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
    lines += ["", "Counts: " + "; ".join(f"{key}={value}" for key, value in sorted(counts.items())) + ".", "",
              "Auditor disagreement cells: pending the covered-point list in contracts.json.", "",
              "Red-green demonstration: deferred to canonical QA; no executed demonstration is claimed.", "",
              "Source facts, executed findings and hypotheses are separate in README.md.", ""]
    return "\n".join(lines), counts


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=HERE / "RESULTS.md")
    options = parser.parse_args()
    body, counts = render(records(options.root))
    options.out.write_text(body)
    print("; ".join(f"{key}={value}" for key, value in sorted(counts.items())))
    return exit_code(counts, json.loads((HERE / "contracts.json").read_text()))


def exit_code(counts, contracts):
    points = disagreement_points(contracts["decision"])
    complete = (counts and counts.get("PASS", 0) >= 86 + len(points) and counts.get("N/A", 0) == 1 and
                counts.get("PASS", 0) + counts.get("N/A", 0) == sum(counts.values()))
    bound = (binding_ready(contracts["decision"]) and
             contracts["decision"].get("covered_points_status") == "bound" and
             bool(contracts["decision"].get("covered_points")) and
             all(point.get("status") == "bound" for point in points) and
             all(contracts[key]["status"] == "bound" for key in
                 ("coordinator", "sentinel", "isolation", "update_fixture")))
    return 0 if complete and bound else 1


if __name__ == "__main__":
    sys.exit(main())
