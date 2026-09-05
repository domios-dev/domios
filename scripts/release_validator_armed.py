#!/usr/bin/env python3
"""Decide whether the release manifest validator will actually fire.

The validator's own contract tests read its file. A file on disk is not an armed
workflow: a workflow disabled in GitHub Actions receives no `workflow_run` event
and refuses `workflow_dispatch`, which is how the validator went ten weeks
without running while every contract test stayed green. Each publishing leg
feeds this checker the validator's runtime state so the release fails loudly
instead of publishing unvalidated.
"""

import json
import re
import sys
from pathlib import Path

WORKFLOW_RUN_KEY = re.compile(r"^ {2}workflow_run:\s*$")
WORKFLOWS_KEY = re.compile(r"^ {4}workflows:\s*$")
LIST_ITEM = re.compile(r"^ {6}-\s+\"?([^\"]+?)\"?\s*$")

ARMED = "VALIDATOR_ARMED"
UNARMED = "VALIDATOR_UNARMED"


def subscribed_workflows(source: str) -> list[str]:
    """The on.workflow_run.workflows names declared by the validator source.

    Parsed without PyYAML so a publishing leg never needs an ambient install.
    """
    names: list[str] = []
    inside_trigger = False
    inside_list = False
    for line in source.splitlines():
        if WORKFLOW_RUN_KEY.match(line):
            inside_trigger = True
            continue
        if not inside_trigger:
            continue
        if line.strip() and not line.startswith("    "):
            break
        if WORKFLOWS_KEY.match(line):
            inside_list = True
            continue
        if not inside_list:
            continue
        item = LIST_ITEM.match(line)
        if item:
            names.append(item.group(1).strip())
        elif line.strip():
            inside_list = False
    return names


def check(fixture: dict) -> tuple[bool, str]:
    validator = fixture.get("validator") or "release-manifest-validate"
    leg = fixture.get("leg")
    branch = fixture.get("default_branch") or "the default branch"
    state = fixture.get("state")
    source = fixture.get("default_branch_source")

    if not leg:
        return False, f"{UNARMED}: {validator} cannot be checked, the calling leg is unnamed"

    if not source:
        return False, (
            f"{UNARMED}: {validator} is absent from {branch}; workflow_run is only "
            f"evaluated from the default-branch copy, so this release would publish "
            f"unvalidated"
        )

    if state is None:
        return False, (
            f"{UNARMED}: {validator} is on {branch} but the Actions API did not report its "
            f"enabled state, so this release cannot show that it will be validated. Check by "
            f"hand: gh workflow list --all --json name,path,state"
        )

    if state != "active":
        return False, (
            f"{UNARMED}: {validator} is {state} in GitHub Actions; a disabled "
            f"workflow receives no workflow_run event and refuses workflow_dispatch, so "
            f"this release would publish unvalidated. "
            f"Re-arm with: gh workflow enable {validator}.yml"
        )

    names = subscribed_workflows(source)
    if not names:
        return False, (
            f"{UNARMED}: {validator} on {branch} declares no on.workflow_run.workflows "
            f"list, so no publishing leg completing can trigger it"
        )

    if leg not in names:
        return False, (
            f"{UNARMED}: {validator} on {branch} does not subscribe to {leg!r} (it watches "
            f"{', '.join(sorted(names))}); this leg completing delivers no event"
        )

    return True, f"{ARMED}: {validator} is active on {branch} and subscribes to {leg!r}"


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(f"usage: {Path(argv[0]).name} <fixture.json>", file=sys.stderr)
        return 2
    try:
        fixture = json.loads(Path(argv[1]).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        print(f"{UNARMED}: armed fixture is unreadable: {error}")
        return 1
    ok, message = check(fixture)
    print(message)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
