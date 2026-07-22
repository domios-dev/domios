import argparse
import json
import os
import queue
import random
import threading
import time
import urllib.error
import urllib.request


API = "http://127.0.0.1:7686"
DEFAULT_TARGETS = ("Goya", "Hooke", "Tesla", "Kelvin")


def request(method, path, body=None, timeout=30):
    token = os.environ["CODETERM_API_TOKEN"]
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        API + path,
        data=data,
        method=method,
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=timeout) as response:
        payload = response.read()
    return json.loads(payload) if payload else None


def message_contains_answer(event, pane_id, answer):
    if event.get("event_type") != f"pane-chat-msg-{pane_id}":
        return False
    messages = event.get("data", {}).get("messages", [])
    return any(
        (msg.get("kind") or msg.get("type")) == "assistant"
        and answer in str(msg.get("content", ""))
        for msg in messages
    )


def is_working(event, pane_id):
    data = event.get("data", {})
    return (
        event.get("event_type") == "pane-status-changed"
        and data.get("pane_id") == pane_id
        and str(data.get("status", "")).lower() == "working"
    )


class EventStream:
    def __init__(self):
        self.events = queue.Queue()
        self.ready = threading.Event()
        self.failure = queue.Queue(maxsize=1)
        self.thread = threading.Thread(target=self._run, daemon=True)

    def start(self):
        self.thread.start()
        if not self.ready.wait(timeout=10):
            raise RuntimeError("SSE stream did not become ready")

    def _run(self):
        token = os.environ["CODETERM_API_TOKEN"]
        req = urllib.request.Request(
            API + "/api/events/stream",
            headers={"Authorization": f"Bearer {token}", "Accept": "text/event-stream"},
        )
        try:
            with urllib.request.urlopen(req, timeout=300) as response:
                self.ready.set()
                for raw in response:
                    line = raw.decode("utf-8", errors="replace").strip()
                    if line.startswith("data:"):
                        self.events.put((time.perf_counter(), json.loads(line[5:].strip())))
        except Exception as error:
            if self.failure.empty():
                self.failure.put(error)
            self.ready.set()


def resolve_targets(names):
    panes = request("GET", "/api/panes")
    by_name = {pane.get("title"): pane for pane in panes}
    missing = [name for name in names if name not in by_name]
    if missing:
        raise RuntimeError(f"missing dev targets: {', '.join(missing)}")
    return [by_name[name] for name in names]


def make_probe(pane, ordinal):
    left = random.randint(2000, 8000) + ordinal * 17
    right = random.randint(2000, 8000) + ordinal * 31
    return {
        "name": pane["title"],
        "pane_id": pane["id"],
        "provider": pane.get("agent_type"),
        "answer": str(left + right),
        "prompt": f"Compute {left} + {right}. Reply with the decimal digits only.",
        "initial_state": pane.get("agent_state", {}).get("state"),
        "boot_stage": pane.get("boot_stage"),
    }


def run(names, deadline, output):
    probes = [make_probe(pane, index) for index, pane in enumerate(resolve_targets(names))]
    stream = EventStream()
    stream.start()

    for probe in probes:
        started = time.perf_counter()
        snapshot = request("POST", f"/api/panes/{probe['pane_id']}/chat-raw/start", {})
        probe["attach_ms"] = round((time.perf_counter() - started) * 1000, 1)
        probe["snapshot_messages"] = len(snapshot.get("messages", []))

    for probe in probes:
        probe["sent_at"] = time.perf_counter()
        request("POST", f"/api/panes/{probe['pane_id']}/send", {"text": probe["prompt"]})
        probe["send_return_ms"] = round((time.perf_counter() - probe["sent_at"]) * 1000, 1)

    pending = {probe["pane_id"]: probe for probe in probes}
    stop_at = time.perf_counter() + deadline
    while pending:
        remaining = stop_at - time.perf_counter()
        if remaining <= 0:
            break
        try:
            observed_at, event = stream.events.get(timeout=remaining)
        except queue.Empty:
            break
        for pane_id, probe in list(pending.items()):
            if "working_ms" not in probe and is_working(event, pane_id):
                probe["working_ms"] = round((observed_at - probe["sent_at"]) * 1000, 1)
            if message_contains_answer(event, pane_id, probe["answer"]):
                probe["reply_ms"] = round((observed_at - probe["sent_at"]) * 1000, 1)
                pending.pop(pane_id)

    for probe in probes:
        probe.setdefault("working_ms", None)
        probe.setdefault("reply_ms", None)
        probe["ok"] = probe["reply_ms"] is not None

    result = {"build": request("GET", "/api/health").get("build"), "probes": probes}
    rendered = json.dumps(result, indent=2)
    if output:
        os.makedirs(os.path.dirname(output), exist_ok=True)
        with open(output, "w", encoding="utf-8") as handle:
            handle.write(rendered + "\n")
    print(rendered)
    return 0 if all(probe["ok"] for probe in probes) else 1


def self_test():
    event = {
        "event_type": "pane-chat-msg-p1",
        "data": {"messages": [{"type": "assistant", "content": "9137"}]},
    }
    assert message_contains_answer(event, "p1", "9137")
    assert not message_contains_answer(event, "p1", "Compute 4000")
    status = {"event_type": "pane-status-changed", "data": {"pane_id": "p1", "status": "Working"}}
    assert is_working(status, "p1")
    print("self-test passed")
    return 0


def main():
    parser = argparse.ArgumentParser(description="Measure restored Chat UI agent response latency")
    parser.add_argument("--targets", nargs="+", default=list(DEFAULT_TARGETS))
    parser.add_argument("--deadline", type=float, default=60.0)
    parser.add_argument(
        "--output",
        default=os.path.join("target-check", "evals", "chat-resume-latency-latest.json"),
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    targets = [name for value in args.targets for name in value.split(",") if name]
    return self_test() if args.self_test else run(targets, args.deadline, args.output)


if __name__ == "__main__":
    raise SystemExit(main())
