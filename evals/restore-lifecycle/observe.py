"""Dev-only observations for the restore lifecycle bench.

Every wait re-evaluates its condition on each daemon event and gives up at a
failure deadline; elapsed time never counts as success.
"""
import json
import os
import pathlib
import queue
import sys
import threading
import time
import urllib.error
import urllib.request

BASE = "http://127.0.0.1:7686"
AGENT_FIELDS = ("agent_type", "agent_session_id", "role", "role_profile", "team_id", "parent_tab_id", "title")


def fail(message):
    print(f"FAIL {message}", flush=True)
    raise SystemExit(1)


def headers():
    token = os.environ.get("CODETERM_API_TOKEN")
    if not token:
        fail("observe.py must run under dev_run (no CODETERM_API_TOKEN)")
    return {"Authorization": f"Bearer {token}"}


def api(method, path, payload=None):
    body = None if payload is None else json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        BASE + path,
        data=body,
        headers={**headers(), "Content-Type": "application/json"},
        method=method,
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        raw = response.read().decode("utf-8")
        return json.loads(raw) if raw.strip() else {}


def status(tab_id):
    try:
        return api("GET", f"/api/tabs/{tab_id}/status")
    except urllib.error.HTTPError:
        return None


class EventWait:
    """Subscribe before acting so no event between action and wait is lost."""

    def __init__(self, deadline_secs):
        self.deadline = time.monotonic() + deadline_secs
        self.events = queue.Queue()
        self.ready = threading.Event()
        self.error = None
        threading.Thread(target=self._stream, daemon=True).start()
        if not self.ready.wait(timeout=min(15, deadline_secs)):
            fail("event stream did not acknowledge subscription")
        if self.error:
            fail(f"event stream failed: {self.error}")

    def _stream(self):
        request = urllib.request.Request(
            BASE + "/api/events/stream",
            headers={**headers(), "Accept": "text/event-stream"},
        )
        try:
            with urllib.request.urlopen(request, timeout=max(1, self.deadline - time.monotonic())) as response:
                self.ready.set()
                for raw in response:
                    line = raw.decode("utf-8", errors="replace").strip()
                    if line.startswith("data:"):
                        try:
                            self.events.put(json.loads(line[5:].strip()))
                        except json.JSONDecodeError:
                            continue
        except Exception as error:  # noqa: BLE001 - surfaced through the wait
            self.error = error
            self.ready.set()
            self.events.put(None)

    def until(self, predicate, what):
        while True:
            verdict = predicate(None)
            if verdict:
                return verdict
            remaining = self.deadline - time.monotonic()
            if remaining <= 0:
                fail(f"deadline elapsed waiting for {what}")
            try:
                event = self.events.get(timeout=remaining)
            except queue.Empty:
                verdict = predicate(None)
                if verdict:
                    return verdict
                fail(f"deadline elapsed waiting for {what}")
            if event is None:
                fail(f"event stream closed waiting for {what}: {self.error}")
            if event.get("event_type") == "overflow":
                verdict = predicate(None)
                if verdict:
                    return verdict
                continue
            verdict = predicate(event)
            if verdict:
                return verdict


def cmd_wait_bound(deadline, tabs):
    wait = EventWait(deadline)

    def bound(_event):
        pending = [t for t in tabs if not ((status(t) or {}).get("agent_session_id") and (status(t) or {}).get("agent_type"))]
        return not pending

    wait.until(bound, f"session binding on {' '.join(tabs)}")
    print("bound " + " ".join(tabs))


def chat_has(tab_id, marker):
    try:
        payload = api("GET", f"/api/tabs/{tab_id}/chat")
    except urllib.error.HTTPError:
        return False
    return marker in json.dumps(payload.get("messages", []), ensure_ascii=False)


def cmd_send_marker(deadline, tab_id, marker, prompt):
    wait = EventWait(deadline)
    api("POST", f"/api/tabs/{tab_id}/chat-raw/start", {"subscriptionId": f"restore-lifecycle-{tab_id}-{marker}"})
    api("POST", f"/api/tabs/{tab_id}/send", {"text": prompt, "enter": True})
    event_name = f"tab-chat-msg-{tab_id}"

    def replied(event):
        if event is None:
            return chat_has(tab_id, marker)
        return event.get("event_type") == event_name and marker in json.dumps(event.get("data"), ensure_ascii=False)

    wait.until(replied, f"marker {marker} from tab {tab_id}")
    print(f"marker {tab_id} {marker}")


def cmd_send_long_turn(deadline, tab_id, prompt):
    wait = EventWait(deadline)
    api("POST", f"/api/tabs/{tab_id}/send", {"text": prompt, "enter": True})

    def working(_event):
        return (status(tab_id) or {}).get("status") == "Working"

    wait.until(working, f"tab {tab_id} to report Working")
    print(f"working {tab_id}")


def cmd_capture(out_path, tabs):
    records = {}
    for tab_id in tabs:
        record = status(tab_id)
        if record is None:
            fail(f"capture: tab {tab_id} not found")
        records[tab_id] = {field: record.get(field) for field in AGENT_FIELDS}
    pathlib.Path(out_path).write_text(json.dumps(records, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(records, sort_keys=True))


def cmd_assert_identity(expected_path, actual_path, shells):
    expected = json.loads(pathlib.Path(expected_path).read_text(encoding="utf-8"))
    actual = json.loads(pathlib.Path(actual_path).read_text(encoding="utf-8"))
    failures = []
    seen_sids = {}
    for tab_id, want in expected.items():
        got = actual.get(tab_id)
        if got is None:
            failures.append(f"tab {tab_id} ({want.get('title')}) missing after restart")
            continue
        for field in AGENT_FIELDS:
            if want.get(field) != got.get(field):
                failures.append(f"tab {tab_id} ({want.get('title')}) {field}: {want.get(field)!r} -> {got.get(field)!r}")
        if tab_id in shells:
            if got.get("agent_type") or got.get("agent_session_id"):
                failures.append(f"shell tab {tab_id} gained an agent identity {got.get('agent_type')!r}/{got.get('agent_session_id')!r}")
            continue
        if not got.get("agent_type") or not got.get("agent_session_id"):
            failures.append(f"agent tab {tab_id} ({want.get('title')}) restored without provider/session")
        sid = got.get("agent_session_id")
        if sid and sid in seen_sids:
            failures.append(f"session {sid} bound to both {seen_sids[sid]} and {tab_id}")
        seen_sids[sid] = tab_id
    if failures:
        for line in failures:
            print(f"FAIL identity {line}")
        raise SystemExit(1)
    print(f"PASS identity {len(expected)} tabs unchanged")


def cmd_assert_pre(expected_path, shells, providers):
    records = json.loads(pathlib.Path(expected_path).read_text(encoding="utf-8"))
    wanted = dict(item.split("=", 1) for item in providers)
    failures = []
    for tab_id, record in records.items():
        if tab_id in shells:
            if record.get("agent_type"):
                failures.append(f"shell {tab_id} has agent_type {record.get('agent_type')!r} before restart")
            continue
        if record.get("agent_type") != wanted.get(tab_id):
            failures.append(f"tab {tab_id} ({record.get('title')}) provider {record.get('agent_type')!r}, spawned as {wanted.get(tab_id)!r}")
        if not record.get("agent_session_id"):
            failures.append(f"tab {tab_id} ({record.get('title')}) has no bound session before restart")
    if failures:
        for line in failures:
            print(f"FAIL pre {line}")
        raise SystemExit(1)
    print(f"PASS pre {len(records)} tabs captured with expected providers")


def main():
    if len(sys.argv) < 2:
        fail("usage: observe.py <command> ...")
    command, args = sys.argv[1], sys.argv[2:]
    if command == "wait-bound":
        cmd_wait_bound(int(args[0]), args[1:])
    elif command == "send-marker":
        cmd_send_marker(int(args[0]), args[1], args[2], args[3])
    elif command == "send-long-turn":
        cmd_send_long_turn(int(args[0]), args[1], args[2])
    elif command == "capture":
        cmd_capture(args[0], args[1:])
    elif command == "assert-identity":
        cmd_assert_identity(args[0], args[1], set(filter(None, args[2].split(","))))
    elif command == "assert-pre":
        cmd_assert_pre(args[0], set(filter(None, args[1].split(","))), args[2:])
    elif command == "restore-report":
        import restore_report

        restore_report.assert_settled(args, EventWait, fail)
    else:
        fail(f"unknown command {command}")


if __name__ == "__main__":
    main()
