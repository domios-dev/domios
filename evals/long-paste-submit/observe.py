"""Observe bound-session delivery without terminal or timing heuristics.

The oracle is a fresh exact user row plus delivered ledger receipt under the
same nonce. PTY write evidence is checked separately by assert_outcome.py.
"""

import argparse
import json
import os
from pathlib import Path
import queue
import threading
import time
import urllib.parse
import urllib.request


def binding(tab):
    nonce = tab.get("session_binding_nonce")
    backing = tab.get("backing", {})
    session = tab.get("agent_session_id") or backing.get("session_id")
    if not nonce or not session or tab.get("reported_session_binding_nonce") != nonce:
        raise ValueError("exact acknowledged own-session binding is unavailable")
    if backing.get("kind") not in ("pty", "chat_backend"):
        raise ValueError("unsupported tab backing")
    if backing.get("kind") == "chat_backend" and backing.get("session_id") != session:
        raise ValueError("runtime thread disagrees with tab session")
    if backing.get("kind") == "pty" and not tab.get("pty_id"):
        raise ValueError("PTY backing has no exact PTY identity")
    return {"session": session, "nonce": nonce, "backing": backing.get("kind"),
            "provider": tab.get("agent_type"), "pty": tab.get("pty_id")}


class Observer:
    def __init__(self, timeout):
        self.base = os.environ["CODETERM_API_URL"].rstrip("/")
        target = urllib.parse.urlparse(self.base)
        if target.hostname != "127.0.0.1" or target.port == 7685 or target.scheme != "http":
            raise ValueError("only bench-lib resolved local DEV is authorized")
        self.headers = {"Authorization": "Bearer " + os.environ["CODETERM_API_TOKEN"]}
        self.timeout = timeout
        self.events = queue.Queue()
        self.ready = threading.Event()
        self.error = None
        threading.Thread(target=self.read_events, daemon=True).start()
        if not self.ready.wait(timeout) or self.error:
            raise RuntimeError(f"event subscription failed: {self.error}")

    def get(self, path):
        request = urllib.request.Request(self.base + path, headers=self.headers)
        with urllib.request.urlopen(request, timeout=self.timeout) as response:
            return json.load(response)

    def read_events(self):
        try:
            request = urllib.request.Request(self.base + "/api/events/stream", headers=self.headers)
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                self.ready.set()
                for line in response:
                    if line.startswith(b"data:"):
                        self.events.put(json.loads(line[5:]))
        except Exception as error:
            self.error = error
            self.ready.set()
            self.events.put({"error": str(error)})

    def wait(self, deadline, tab):
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("delivery observation deadline")
            event = self.events.get(timeout=remaining)
            if "error" in event:
                raise RuntimeError(event["error"])
            kind = event.get("event_type", "")
            data = event.get("data")
            data = data if isinstance(data, dict) else {}
            if kind in (f"tab-chat-{tab}", f"tab-chat-msg-{tab}") or data.get("tab_id", data.get("id")) == tab:
                return

    def inspect(self, tab):
        rows = self.get("/api/tabs/inspect?tab_ids=" + urllib.parse.quote(tab))
        if len(rows) != 1:
            raise ValueError("expected exactly one inspected tab")
        return rows[0]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("ready", "anchor", "settle"))
    parser.add_argument("tab")
    parser.add_argument("directory", type=Path)
    parser.add_argument("--timeout", type=int, default=90)
    parser.add_argument("--send", action="append", default=[])
    parser.add_argument("--payload", action="append", type=Path, default=[])
    args = parser.parse_args()
    observer = Observer(args.timeout)
    deadline = time.monotonic() + args.timeout
    prefix = f"/api/tabs/{args.tab}"
    if args.mode == "ready":
        while True:
            tab = observer.inspect(args.tab)
            status = observer.get(prefix + "/status").get("status", "").lower()
            if status == "idle" and tab.get("boot_stage") == "agent_ready":
                binding(tab)
                return
            observer.wait(deadline, args.tab)
    anchor_path = args.directory / "evidence-anchor.json"
    if args.mode == "anchor":
        own = binding(observer.inspect(args.tab))
        transcript = observer.get(prefix + "/chat")
        if binding(observer.inspect(args.tab)) != own:
            raise ValueError("binding changed while capturing append boundary")
        anchor = {**own, "binding": own, "count": transcript["count"],
                  "before_ids": [row["id"] for row in transcript["messages"]]}
        anchor_path.write_text(json.dumps(anchor, indent=2), encoding="utf8")
        return
    anchor = json.loads(anchor_path.read_text(encoding="utf8"))
    if not args.send or len(args.send) != len(args.payload):
        raise ValueError("settlement requires paired sends and payloads")
    payloads = [path.read_text(encoding="utf8").strip() for path in args.payload]
    while True:
        own = binding(observer.inspect(args.tab))
        if own != anchor["binding"]:
            raise ValueError("own-session authority changed during delivery")
        ledger = observer.get(prefix + "/chat-send")
        transcript = observer.get(prefix + "/chat?since=" + str(anchor["count"]))
        after = binding(observer.inspect(args.tab))
        (args.directory / "evidence-binding-after.json").write_text(json.dumps(after), encoding="utf8")
        (args.directory / "assert-transcript.json").write_text(json.dumps(transcript), encoding="utf8")
        (args.directory / "assert-ledger.json").write_text(json.dumps(ledger), encoding="utf8")
        if after != own:
            raise ValueError("binding changed during evidence read")
        delivered = all(len([row for row in ledger if row.get("send_id") == send
                            and row.get("state") == "delivered"]) == 1 for send in args.send)
        messages = transcript["messages"]
        consumed = all(any(row.get("type") == "user" and row.get("content", "").strip() == payload
                           and row.get("id") not in anchor["before_ids"]
                           and not row.get("inheritedFromSessionId")
                           and any(reply.get("type") == "assistant" and reply.get("content")
                                   for reply in messages[index + 1:])
                           for index, row in enumerate(messages)) for payload in payloads)
        if delivered and consumed:
            return
        observer.wait(deadline, args.tab)


if __name__ == "__main__":
    main()
