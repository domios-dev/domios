import json
import sys
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from release_issue_resolution import included_history, reachable_candidates

fixture = json.load(sys.stdin)
calls = []


def request(route, raw=False):
    calls.append({"route": route, "raw": raw})
    if route not in fixture["responses"]:
        raise AssertionError("unexpected remote call: " + route)
    value = fixture["responses"][route]
    return json.dumps(value) if raw else value


def commits(start, source, limit):
    assert start == fixture["expected_start"], (start, fixture["expected_start"])
    assert source == fixture["sha"]
    return fixture["commits"]


try:
    if "tags" in fixture:
        def git_output(args):
            calls.append(args)
            if args[0] == "tag":
                return "\n".join(fixture["tags"])
            assert args[0] == "rev-parse"
            return "\n".join(fixture["objects"][ref] for ref in args[1:])

        with patch("release_issue_resolution.git_output", git_output):
            result = reachable_candidates(fixture["sha"])
    else:
        result = included_history(fixture["version"], fixture["sha"], fixture["candidates"], commits, request)
    print(json.dumps({"result": result, "calls": calls}))
except Exception as error:
    print(json.dumps({"error": str(error), "calls": calls}))
