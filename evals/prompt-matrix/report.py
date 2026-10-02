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
    points = json.loads((HERE / "contracts.json").read_text())["decision"]["covered_points"]
    for point in points:
        yield point["provider"], point["case"], "on", "disagreement", point["id"]


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
                judge(result["evidence"])
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
    lines = [
        "# Provider prompt matrix", "",
        "Release gate remains closed until every required cell passes or has explicit owner acceptance.",
        "Missing evidence is NOT RUN. Pending contracts are PENDING. Neither is a pass.", "",
        "Off is the heuristic arm. On exercises connected Jev primary and records any actual fallback.",
        "The deciding path comes only from `decided_by` plus `observation_id`, never from Jev configuration.", "",
        "| Provider | Case | Jev | Variant | Decision point | Result | Deciding path | Transcript / finding |",
        "|---|---|---|---|---|---|---|---|",
    ]
    counts = Counter()
    for cell in cells():
        result = results.get(cell, {})
        status = result.get("status", "NOT RUN")
        counts[status] += 1
        path = result.get("path") or "unobserved"
        artifact = result.get("artifact")
        note = result.get("reason", "No live gate executed")
        if artifact:
            note = f"[transcript]({artifact}/transcript.jsonl): {escaped(note)}"
        lines.append("| " + " | ".join(map(escaped, (*cell, status, path, note))) + " |")
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
    return 0 if counts and counts.get("PASS", 0) == sum(counts.values()) else 1


if __name__ == "__main__":
    sys.exit(main())
