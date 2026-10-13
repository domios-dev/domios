import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import threading
import time
import urllib.request
import uuid

from model import matching_records, summarize


class Events:
    def __init__(self, url, headers, timeout):
        self.queue = queue.Queue()
        self.ready = threading.Event()
        self.error = None
        self.url, self.headers, self.timeout = url, headers, timeout
        threading.Thread(target=self.read, daemon=True).start()
        if not self.ready.wait(min(timeout, 10)) or self.error:
            raise RuntimeError(f"event subscription failed: {self.error}")

    def read(self):
        try:
            request = urllib.request.Request(self.url, headers=self.headers)
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                self.ready.set()
                for line in response:
                    if line.startswith(b"data:"):
                        self.queue.put(json.loads(line[5:]))
        except Exception as error:
            self.error = error
            self.ready.set()
            self.queue.put({"error": str(error)})

    def wait(self, deadline, tab):
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("delivery event deadline expired")
            event = self.queue.get(timeout=remaining)
            if "error" in event:
                raise RuntimeError(event["error"])
            kind, data = event.get("event_type", ""), event.get("data") or {}
            if (kind == f"tab-chat-{tab}" or
                    data.get("tab_id", data.get("id")) == tab and kind in
                    ("chat-send-status", "tab-status-changed", "tab-state-surface")):
                return


def run(args):
    base = os.environ["CODETERM_API_URL"]
    if not base.startswith("http://127.0.0.1:") or base.endswith(":7685"):
        raise RuntimeError("only the bench-lib resolved local DEV is authorized")
    headers = {"Authorization": f"Bearer {os.environ['CODETERM_API_TOKEN']}"}
    script = Path(__file__).with_name("run.sh")

    def cli(*command, input_text=None):
        result = subprocess.run(["bash", str(script), "--dc", *command],
                                capture_output=True, text=True, input=input_text, timeout=args.timeout)
        if result.returncode:
            raise RuntimeError(f"dc {command[0]} exit {result.returncode}: {result.stderr}")
        return json.loads(result.stdout)

    def get(path):
        request = urllib.request.Request(base + path, headers=headers)
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)

    def inspect():
        tab = cli("tab", "inspect", "--tab", args.tab, "--json")[0]
        if (tab.get("agent_type") != args.provider or
                tab.get("agent_session_id") != args.session or
                tab.get("session_binding_nonce") != tab.get("reported_session_binding_nonce") or
                tab.get("backing", {}).get("kind") != "pty"):
            raise RuntimeError("target provider/own-session binding changed or is unverified")
        return tab

    def phase():
        return get(f"/api/tabs/{args.tab}/status").get("status", "").lower()

    events = Events(base + "/api/events/stream", headers, args.timeout)
    inspect()
    if not args.transcript.is_file():
        raise RuntimeError("the exact claimed-session append-only transcript must exist")
    nonce = inspect().get("session_binding_nonce")
    pad = Path(cli("--as-tab", args.caller, "pad", "path", "--json")["path"])
    run_id = uuid.uuid4().hex
    args.output.mkdir(parents=True, exist_ok=True)
    rows = []
    try:
        for index in range(10):
            deadline = time.monotonic() + args.timeout
            expected_phase = "idle" if args.mode == "idle" else "working"
            while phase() != expected_phase:
                if args.mode == "busy":
                    raise RuntimeError("busy group requires an already working agent before every send")
                events.wait(deadline, args.tab)
            if inspect().get("session_binding_nonce") != nonce:
                raise RuntimeError("launch changed during drill")
            statuses_before = get(f"/api/tabs/{args.tab}/chat-send")
            before_ids = {row["send_id"] for row in statuses_before}
            anchor = args.transcript.stat().st_size
            payload = f"delivery-confirmation {run_id} {index}: reply with this token only."
            filename = f"delivery-confirmation-{run_id}-{index}.txt"
            (pad / filename).write_text(payload, encoding="utf8")
            print(json.dumps({"index": index, "payload": payload, "pad_file": filename,
                              "anchor": anchor}), flush=True)
            if args.path == "cli":
                cli("send", "--tab", args.tab, "--from-pad", "-", "--require-agent", "--json",
                    input_text=(pad / filename).read_text(encoding="utf8"))
            else:
                print("Submit this exact payload once through the visible Chat composer.", flush=True)
            while True:
                if inspect().get("session_binding_nonce") != nonce:
                    raise RuntimeError("launch changed while awaiting this send")
                statuses = get(f"/api/tabs/{args.tab}/chat-send")
                new = [row for row in statuses if row["send_id"] not in before_ids]
                if len(new) > 1:
                    raise RuntimeError("concurrent input invalidates send correlation on this dedicated tab")
                evidence = matching_records(args.transcript, anchor, args.session, payload)
                if args.mode == "busy" and new:
                    state = new[0]["state"]
                    break
                if new and new[0]["state"] in ("delivered", "submit_unconfirmed", "held", "failed"):
                    state = new[0]["state"]
                    if state == "delivered" and evidence or state == "failed":
                        break
                    if phase() == "idle" and state != "held":
                        break
                try:
                    events.wait(deadline, args.tab)
                except (TimeoutError, queue.Empty):
                    if not new:
                        raise RuntimeError("no correlated ledger receipt before the failure deadline")
                    state = new[0]["state"]
                    break
            row = {"provider": args.provider, "path": args.path, "mode": args.mode,
                   "tab_id": args.tab, "session_id": args.session, "launch_nonce": nonce,
                   "index": index, "payload": payload, "anchor": anchor,
                   "send_id": new[0]["send_id"], "state": state, "evidence": evidence}
            rows.append(row)
            (args.output / f"{run_id}.json").write_text(json.dumps(rows, indent=2))
            if args.mode == "busy" and index < 9 and phase() != "working":
                raise RuntimeError("agent finished before all ten busy sends; this group is incomplete")
        if args.mode == "busy":
            deadline = time.monotonic() + args.timeout
            while True:
                if inspect().get("session_binding_nonce") != nonce:
                    raise RuntimeError("launch changed during settlement")
                statuses = {row["send_id"]: row for row in get(f"/api/tabs/{args.tab}/chat-send")}
                for row in rows:
                    row["state"] = statuses.get(row["send_id"], {}).get("state", "absent")
                    row["evidence"] = matching_records(args.transcript, row["anchor"], args.session, row["payload"])
                if all(row["state"] == "delivered" and row["evidence"] for row in rows):
                    break
                if phase() == "idle" and all(row["state"] != "written" for row in rows):
                    break
                try:
                    events.wait(deadline, args.tab)
                except (TimeoutError, queue.Empty):
                    break
            (args.output / f"{run_id}.json").write_text(json.dumps(rows, indent=2))
    finally:
        (args.output / f"{run_id}-summary.json").write_text(json.dumps(summarize(rows), indent=2))
        for path in pad.glob(f"delivery-confirmation-{run_id}-*.txt"):
            path.unlink()
    print(json.dumps(summarize(rows), indent=2))
    return 0 if len(rows) == 10 and all(row["state"] == "delivered" and row["evidence"] for row in rows) else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--provider", choices=["claude", "codex"], required=True)
    parser.add_argument("--tab", required=True)
    parser.add_argument("--session", required=True)
    parser.add_argument("--caller", required=True)
    parser.add_argument("--transcript", type=Path, required=True)
    parser.add_argument("--path", choices=["composer", "cli"], required=True)
    parser.add_argument("--mode", choices=["idle", "busy"], required=True)
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--output", type=Path, default=Path("target-check/evals/delivery-confirmation"))
    raise SystemExit(run(parser.parse_args()))
