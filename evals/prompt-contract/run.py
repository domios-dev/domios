#!/usr/bin/env python3
"""Deterministic prompt-contract and embedded-doc reading fixtures."""

import json
import re
import sys
from hashlib import sha256
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = json.loads(
    Path(__file__).with_name("protocol-fixtures.json").read_text(encoding="utf-8")
)
CONTRACT = ROOT / "src-tauri/prompts/bounce_unwinding_contract.md"
ORCHESTRATION_DOC = ROOT / "docs/ORCHESTRATION.md"


def record_output(record, fields):
    return "\n".join(f"{field}: {record[field]}" for field in fields)


def validate_record(record, fields):
    assert list(record) == fields, f"field order or schema changed: {list(record)}"
    assert record["Baseline target"].strip()
    assert record["Baseline oracle"].strip()
    assert record["Baseline pass/fail"].strip()
    assert record["Hypothesis"].strip()
    assert "repo/fixture" in record["Evidence"]
    assert "—" in record["Evidence"]
    assert record["Budget"] == "inspect at most 5 repo surfaces; consult at most 1 external source"
    assert record["Stop rule"].strip()
    assert record["Keep"].strip() and record["Discard"].strip()
    assert record["Route"].strip() and record["Fixture"].strip()
    assert record["Follow-up"] == "none" or "owner:" in record["Follow-up"]


def assert_baseline_immutable(record, accepted):
    for field in ("Baseline target", "Baseline oracle", "Baseline pass/fail"):
        assert record[field] == accepted[field], f"immutable {field} changed"


def assert_followup_is_bounded(record, open_questions):
    followup = record["Follow-up"]
    if followup == "none":
        return
    assert record["Question"] not in open_questions, "duplicate follow-up question"
    assert "owner:" in followup and "action:" in followup, "follow-up lacks ownership"


def contract_schema(text):
    start = text.index("  ```text\n  Trigger:")
    end = text.index("  ```", start + len("  ```text\n"))
    return text[start + len("  ```text\n"):end]


def assert_prompt_contract():
    shared = CONTRACT.read_text(encoding="utf-8")
    schema = contract_schema(shared)
    fields = [line.split(":", 1)[0].strip() for line in schema.splitlines()]
    assert fields == FIXTURES["required_fields"], "shared protocol schema drifted"
    assert "at most 5 repo surfaces" in schema
    assert "at most 1 external source" in schema
    assert "verbatim" in schema
    assert "`codeterm plan skill new|edit`" in schema
    assert "`codeterm orchestrator learn`" in schema
    assert "`codeterm plan retrospective <plan> --body \"...\"`" in schema


def controlled_mutations(fixtures, fields):
    accepted = fixtures[0]["record"]
    weakened = dict(accepted, **{"Baseline oracle": "cargo test loop_engine"})
    try:
        assert_baseline_immutable(weakened, accepted)
    except AssertionError:
        pass
    else:
        raise AssertionError("oracle-weakening mutation was accepted")

    duplicate = dict(fixtures[3]["record"])
    try:
        assert_followup_is_bounded(duplicate, {duplicate["Question"]})
    except AssertionError:
        pass
    else:
        raise AssertionError("duplicate follow-up mutation was accepted")

    unbounded = dict(accepted, **{"Budget": "investigate more"})
    try:
        validate_record(unbounded, fields)
    except AssertionError:
        pass
    else:
        raise AssertionError("unbounded-budget mutation was accepted")


def section_slugs(document):
    slugs = set()
    for line in document.splitlines():
        if not line.startswith("## "):
            continue
        slug = re.sub(
            r"[^\w]+", "-", line[3:].casefold(), flags=re.UNICODE
        ).strip("-")
        slugs.add(slug)
    return slugs


def validate_docs_reading_response(fixture, response, available_slugs):
    expected = (
        f"codeterm docs {fixture['document']} "
        f"--section {fixture['expected_section']}"
    )
    assert fixture["question"].strip(), "docs-reading fixture needs a question"
    assert fixture["expected_section"] in available_slugs, "fixture section slug drifted"
    assert response == expected, "dispatch question did not choose the exact complete section"
    assert sha256(response.encode()).hexdigest() == fixture["expected_output_sha256"], (
        "docs-reading expected output drifted"
    )
    assert response.count("codeterm docs ") == 1, "docs read must be one invocation"
    assert "--head" not in response, "required instruction read used an incomplete excerpt"
    assert "--full" not in response, "named dispatch section must not trigger a blanket full read"


def assert_docs_reading_contract():
    fixtures = FIXTURES["docs_reading_fixtures"]
    assert [fixture["id"] for fixture in fixtures] == ["D1"]
    slugs = section_slugs(ORCHESTRATION_DOC.read_text(encoding="utf-8"))
    for fixture in fixtures:
        validate_docs_reading_response(fixture, fixture["response"], slugs)

    preview_retry = (
        "codeterm docs ORCHESTRATION\n"
        "codeterm docs ORCHESTRATION --full"
    )
    try:
        validate_docs_reading_response(fixtures[0], preview_retry, slugs)
    except AssertionError:
        pass
    else:
        raise AssertionError("preview-then-full mutation was accepted")


def main():
    fields = FIXTURES["required_fields"]
    fixtures = FIXTURES["fixtures"]
    assert [fixture["id"] for fixture in fixtures] == [f"F{i}" for i in range(1, 11)]
    assert [fixture["role"] for fixture in fixtures] == ["planner"] * 5 + ["orchestrator"] * 5
    assert_prompt_contract()
    for fixture in fixtures:
        record = fixture["record"]
        validate_record(record, fields)
        actual = record_output(record, fields).encode()
        assert sha256(actual).hexdigest() == fixture["expected_output_sha256"], (
            f"{fixture['id']} exact expected output drifted"
        )
    assert_baseline_immutable(fixtures[1]["record"], fixtures[0]["record"])
    assert_baseline_immutable(fixtures[7]["record"], fixtures[0]["record"])
    assert_followup_is_bounded(fixtures[3]["record"], set())
    controlled_mutations(fixtures, fields)
    assert_docs_reading_contract()
    print("prompt-contract F1-F10 records, D1 docs read, and mutation negatives passed")


if __name__ == "__main__":
    try:
        main()
    except (AssertionError, KeyError, ValueError) as error:
        print(f"FAIL prompt-contract: {error}", file=sys.stderr)
        raise SystemExit(1)
