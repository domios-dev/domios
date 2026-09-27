# WHAT: Wait for a tab event and inspect its current chat snapshot.
# WHY: Event-driven reads avoid timer polling and repeated chat subscriptions.
# ORACLE: Matching snapshot content or an observed idle interval decides the wait.

import json
import os
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen


API_URL = os.environ["WAIT_API_URL"].rstrip("/")
AUTH = os.environ["WAIT_AUTH_HEADER"]
TAB_ID = os.environ["WAIT_TAB_ID"]
MODE = os.environ["WAIT_MODE"]
SUBSCRIPTION_ID = os.environ["WAIT_SUBSCRIPTION_ID"]
SECONDS = int(os.environ["WAIT_SECONDS"])
NEEDLES = json.loads(os.environ["WAIT_NEEDLES"])
DEADLINE = time.monotonic() + SECONDS
LAST_SNAPSHOT = None


def request_json(path, method="GET", body=None):
    header_name, header_value = AUTH.split(":", 1)
    headers = {header_name: header_value.strip()}
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    remaining = DEADLINE - time.monotonic()
    if remaining <= 0:
        raise TimeoutError(f"wait deadline expired before reading {path}")
    request = Request(f"{API_URL}{path}", data=data, headers=headers, method=method)
    try:
        with urlopen(request, timeout=min(3.0, remaining)) as response:
            return json.loads(response.read())
    except HTTPError as error:
        body_text = error.read().decode(errors="replace")
        raise RuntimeError(f"HTTP {error.code} {path}: {body_text}") from error
    except URLError as error:
        raise RuntimeError(f"{path}: {error}") from error


def chat_snapshot(initial):
    encoded_tab = quote(TAB_ID, safe="")
    if initial and (MODE.startswith("chat_start") or MODE == "idle"):
        return request_json(
            f"/api/tabs/{encoded_tab}/chat-raw/start",
            method="POST",
            body={"subscriptionId": SUBSCRIPTION_ID},
        )
    return request_json(f"/api/tabs/{encoded_tab}/chat?since=0")


def session_binding_ready():
    global LAST_SNAPSHOT
    LAST_SNAPSHOT = request_json("/api/tabs")
    rows = LAST_SNAPSHOT if isinstance(LAST_SNAPSHOT, list) else LAST_SNAPSHOT.get("tabs", [])
    tab = next((row for row in rows if row.get("id") == TAB_ID), None)
    if tab is None:
        return False
    session_id = tab.get("agent_session_id")
    nonce = tab.get("session_binding_nonce")
    reported_nonce = tab.get("reported_session_binding_nonce")
    return bool(session_id and nonce and reported_nonce == nonce)


def tool_stats(value):
    names = []

    def walk(node):
        if isinstance(node, dict):
            kind = str(node.get("type") or node.get("message_type") or "")
            name = str(node.get("tool_name") or node.get("tool") or node.get("name") or "")
            if "tool" in kind.lower() or name:
                if name:
                    names.append(name.lower())
            for child in node.values():
                walk(child)
        elif isinstance(node, list):
            for child in node:
                walk(child)

    walk(value)
    reads = [name for name in names if name in {"read", "grep", "glob", "ls"}]
    return len(names), len(reads)


def inspect_snapshot(initial=False):
    global LAST_SNAPSHOT
    if time.monotonic() >= DEADLINE:
        raise TimeoutError("wait deadline expired while reading a chat snapshot")
    if MODE == "session_binding":
        ready = session_binding_ready()
        if time.monotonic() >= DEADLINE:
            raise TimeoutError("wait deadline expired while reading the Tab session binding")
        if not ready:
            return False
        return True
    LAST_SNAPSHOT = chat_snapshot(initial)
    if time.monotonic() >= DEADLINE:
        raise TimeoutError("wait deadline expired while reading a chat snapshot")
    serialized = json.dumps(LAST_SNAPSHOT, ensure_ascii=False)
    if MODE == "idle":
        calls, reads = tool_stats(LAST_SNAPSHOT)
        if calls or reads:
            raise RuntimeError(
                f"planner acted before a goal: tool_calls={calls} file_reads={reads}; "
                f"snapshot={serialized}"
            )
        return False
    if MODE == "chat_start_all":
        return all(needle in serialized for needle in NEEDLES)
    return any(needle in serialized for needle in NEEDLES)


def read_response_headers():
    status = ""
    while True:
        raw = sys.stdin.buffer.readline()
        if not raw:
            raise RuntimeError("event stream ended before response headers")
        line = raw.decode(errors="replace").rstrip("\r\n")
        if line.startswith("HTTP/"):
            status = line
        if not line:
            break
    if " 200 " not in status:
        raise RuntimeError(f"event stream returned {status or 'no HTTP status'}")


def event_blocks():
    fields = []
    for raw in sys.stdin.buffer:
        line = raw.decode(errors="replace").rstrip("\r\n")
        if line:
            if line.startswith("data:"):
                fields.append(line[5:].lstrip())
            continue
        if fields:
            yield "\n".join(fields)
            fields = []
    if fields:
        yield "\n".join(fields)


def main():
    try:
        read_response_headers()
        if inspect_snapshot(initial=True):
            print(f"MATCH: initial {MODE} snapshot", flush=True)
            return 0
        for payload in event_blocks():
            if time.monotonic() >= DEADLINE:
                raise TimeoutError("wait deadline expired before the next tab event")
            event = json.loads(payload)
            if event.get("event_type") == "overflow":
                raise RuntimeError(f"event stream reported lost events: {payload}")
            data = event.get("data") or {}
            if MODE == "session_binding":
                if (
                    event.get("event_type") != "session-binding-state-changed"
                    or data.get("tab_id") != TAB_ID
                    or data.get("kind") != "healthy"
                    or not data.get("current_session_id")
                ):
                    continue
                if inspect_snapshot():
                    print("MATCH: verified session-binding event", flush=True)
                    return 0
                continue
            is_tab_event = data.get("tab_id") == TAB_ID or str(
                event.get("event_type", "")
            ).endswith(f"-{TAB_ID}")
            if not is_tab_event:
                continue
            if inspect_snapshot():
                print("MATCH: tab event satisfied chat condition", flush=True)
                return 0
        if MODE == "idle":
            print("QUIET_WINDOW_COMPLETE", flush=True)
            return 0
        snapshot = json.dumps(LAST_SNAPSHOT, ensure_ascii=False)
        raise TimeoutError(f"no matching tab event before deadline; last snapshot={snapshot}")
    except Exception as error:
        print(f"WAIT_ERROR: {error}", file=sys.stderr, flush=True)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
