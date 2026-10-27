import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from release_issue_resolution import included_history

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
    result = included_history(fixture["version"], fixture["sha"], fixture["candidates"], commits, request)
    print(json.dumps({"result": result, "calls": calls}))
except Exception as error:
    print(json.dumps({"error": str(error), "calls": calls}))
