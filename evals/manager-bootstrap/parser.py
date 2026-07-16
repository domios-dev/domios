#!/usr/bin/env python3
import json
import os
import sys
import urllib.request


def fail(message):
    print(f"manager-bootstrap parser: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_stdin():
    try:
        return json.load(sys.stdin)
    except json.JSONDecodeError as error:
        fail(f"invalid JSON: {error.msg}")


def text(value):
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "".join(text(item) for item in value)
    if isinstance(value, dict):
        for key in ("text", "content", "value"):
            if key in value:
                return text(value[key])
    return ""


def first_assistant(payload):
    messages = payload if isinstance(payload, list) else payload.get("messages") if isinstance(payload, dict) else None
    if not isinstance(messages, list):
        fail("chat envelope must be a message array or an object with messages")
    for message in messages:
        if not isinstance(message, dict):
            continue
        role = message.get("role", message.get("author", message.get("type", "")))
        if isinstance(role, str) and role.lower() == "assistant":
            return text(message.get("content", message.get("text", "")))
    return ""


def provider_id(kind):
    providers = load_stdin()
    if not isinstance(providers, list):
        fail("agent providers envelope must be a top-level array")
    external = kind == "external"
    for provider in providers:
        if not isinstance(provider, dict):
            fail("agent providers array contains a non-object entry")
        if provider.get("system_prompt_delivery") == "external" if external else provider.get("system_prompt_delivery") != "external":
            identifier = provider.get("id")
            if isinstance(identifier, str) and identifier:
                print(identifier)
                return
    print("")


def team_id(before_raw):
    try:
        before = json.loads(before_raw)
    except json.JSONDecodeError as error:
        fail(f"invalid prior team JSON: {error.msg}")
    after = load_stdin()
    if not isinstance(before, dict) or not isinstance(before.get("teams"), list):
        fail("prior team-list envelope must be an object with teams")
    if not isinstance(after, dict) or not isinstance(after.get("teams"), list):
        fail("team-list envelope must be an object with teams")
    before_ids = {team.get("id") for team in before["teams"] if isinstance(team, dict)}
    fresh = [team for team in after["teams"] if isinstance(team, dict) and team.get("id") not in before_ids]
    if len(fresh) != 1 or not isinstance(fresh[0].get("id"), str) or not fresh[0]["id"]:
        fail("expected exactly one new team")
    print(fresh[0]["id"])


def field(name):
    payload = load_stdin()
    if not isinstance(payload, dict):
        fail(f"{name} envelope must be an object")
    if name == "staffed-pane":
        member = payload.get("member")
        value = payload.get("pane_id") or (member.get("pane_id") if isinstance(member, dict) else "")
    else:
        keys = {"orchestrator": "orchestrator_pane_id", "planner": "planner_pane_id", "spawned-pane": "pane_id"}
        value = payload.get(keys[name])
    if not isinstance(value, str) or not value:
        fail(f"{name} envelope has no pane id")
    print(value)


def mux_absent(ids):
    payload = load_stdin()
    if not isinstance(payload, dict):
        fail("mux envelope must be an object")
    panes = payload.get("panes")
    if isinstance(panes, dict):
        pane_ids = set(panes)
    elif isinstance(panes, list):
        pane_ids = {
            pane.get("id")
            for pane in panes
            if isinstance(pane, dict) and isinstance(pane.get("id"), str)
        }
    else:
        fail("mux panes must be an object map or array")
    remaining = sorted(set(ids) & pane_ids)
    if remaining:
        fail(f"mux still contains panes: {', '.join(remaining)}")


def await_event(pane, timeout):
    url = "http://127.0.0.1:7686/api/events/stream"
    request = urllib.request.Request(url, headers={"Authorization": os.environ["CODETERM_DEV_AUTH_HEADER"].removeprefix("Authorization: ")})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            for raw in response:
                if not raw.startswith(b"data: "):
                    continue
                try:
                    event = json.loads(raw[6:])
                except json.JSONDecodeError:
                    continue
                if event.get("event_type") != f"pane-chat-{pane}":
                    continue
                answer = first_assistant(event.get("data", {}))
                if answer:
                    print(answer, end="")
                    return
    except TimeoutError:
        pass
    except Exception as error:
        fail(f"chat event wait failed: {error}")
    fail(f"timeout first assistant {pane}")


def main():
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "first-assistant":
        print(first_assistant(load_stdin()))
    elif command == "provider-id" and len(sys.argv) == 3:
        provider_id(sys.argv[2])
    elif command == "team-id" and len(sys.argv) == 3:
        team_id(sys.argv[2])
    elif command == "field" and len(sys.argv) == 3:
        field(sys.argv[2])
    elif command == "mux-absent" and len(sys.argv) >= 2:
        mux_absent(sys.argv[2:])
    elif command == "await-event" and len(sys.argv) == 4:
        await_event(sys.argv[2], float(sys.argv[3]))
    else:
        fail("invalid parser command")


if __name__ == "__main__":
    main()
