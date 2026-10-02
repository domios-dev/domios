"""WHAT: Bind real Sentinel CLI watches to isolated git and CI provokers.
WHY: Model calls, transitions and delivered receipts need independent evidence.
ORACLE: Bounded process signals and journal records prove outcomes before attribution.
"""

import json
from collections import Counter
import os
from pathlib import Path
import queue
import shlex
import socket
import subprocess
import sys
import threading
import time
import uuid

from fixtures import HERE, write_json
from oracles import load_journal, objects, ready, require
from scenarios import identifier
from sentinel_oracles import judge_sentinel_feed, judge_sentinel_outcome
from transport import Finding, run_group


class ProbeSignals:
    def __init__(self, dev, events):
        self.dev, self.events = dev, events
        self.nonce = uuid.uuid4().hex
        self.socket = socket.socket()
        self.socket.bind(("127.0.0.1", 0))
        self.socket.listen(2)
        self.socket.settimeout(dev.remaining())
        self.port = self.socket.getsockname()[1]
        self.rows = []
        self.lock = threading.Lock()
        self.closed = threading.Event()
        self.thread = threading.Thread(target=self.read, daemon=True)

    def read(self):
        try:
            while not self.closed.is_set():
                connection, address = self.socket.accept()
                with connection:
                    connection.settimeout(min(2, self.dev.remaining()))
                    raw = connection.makefile("rb").readline(4097)
                require(address[0] == "127.0.0.1" and len(raw) <= 4096 and raw.endswith(b"\n"),
                        "Probe signal escaped its bounded local protocol")
                row = json.loads(raw)
                require(row.get("nonce") == self.nonce, "Foreign probe signal")
                with self.lock:
                    require(len(self.rows) < 64, "Probe signal cap exceeded")
                    self.rows.append(row)
                self.dev.transcript.record("fixture_probe", row)
                self.events.frames.put_nowait({"event_type": "fixture-probe", "data": row})
        except Exception as error:
            if not self.closed.is_set():
                self.events.error = error
                try:
                    self.events.frames.put_nowait({"error": str(error)})
                except queue.Full:
                    pass

    def count(self, status):
        with self.lock:
            return sum(row["state"]["status"] == status for row in self.rows)

    def close(self):
        self.closed.set()
        try:
            with socket.create_connection(("127.0.0.1", self.port), timeout=1):
                pass
        except OSError:
            pass
        self.socket.close()
        self.thread.join(timeout=2)


class SentinelScenario:
    def __init__(self, dev, events, root, fixture, options, data_dir):
        self.dev, self.events, self.root = dev, events, root
        self.fixture, self.options, self.data_dir = fixture, options, data_dir
        self.tab = None
        self.watches = []
        self.scopes = {}
        self.restore = {}
        self.signals = None
        self.start_row = len(load_journal(data_dir))
        self.directory = Path(fixture["case_root"])
        self.state = self.directory / "sentinel-state.json"

    def command(self, *args):
        return self.dev.dc("--as-tab", self.tab, "sentinel", *args)

    def set_config(self, key, value):
        if key not in self.restore:
            current = self.dev.dc("config", "get", key)
            require(isinstance(current, dict) and "value" in current,
                    "Config get lacks an exact reversible value")
            self.restore[key] = current["value"]
        self.dev.dc("config", "set", key, json.dumps(value))

    def fixture_command(self, *args):
        self.dev.transcript.record("fixture_command", list(args))
        code, out, err = run_group(list(args), self.dev.env, min(10, self.dev.remaining()))
        require(code == 0, f"Sentinel fixture command failed: {err or out}")
        return out.strip()

    def git_setup(self):
        repo = self.directory / "git"
        self.fixture_command("git", "init", str(repo))
        git = ["git", "-C", str(repo), "-c", "user.name=Matrix", "-c", "user.email=matrix@example.invalid"]
        self.fixture_command(*git, "commit", "--allow-empty", "-m", "base")
        self.fixture_command(*git, "branch", "watched")
        self.fixture_command(*git, "commit", "--allow-empty", "-m", "incoming")
        commit = self.fixture_command(*git, "rev-parse", "HEAD")
        write_json(self.state, {"repo": str(repo), "commit": commit, "branch": "watched"})
        return repo

    def ci_set(self, status):
        argv = [sys.executable, str(HERE / "ci_fixture.py"), "--state", str(self.state), "--set", status]
        self.dev.transcript.record("fixture_command", argv)
        code, out, err = run_group(argv, self.dev.env, min(5, self.dev.remaining()))
        self.dev.transcript.record("fixture_response", {"exit": code, "stdout": out, "stderr": err})
        require(code == {"running": 2, "success": 0, "failure": 1}[status] and
                json.loads(out)["status"] == status, "Fake CI fixture failed to publish its requested status")

    def create(self, buckets, action, matches=True):
        argv = [sys.executable, str(HERE / "sentinel_probe.py"), "--state", str(self.state),
                "--port", str(self.signals.port), "--nonce", self.signals.nonce]
        probe = subprocess.list2cmdline(argv) if os.name == "nt" else shlex.join(argv)
        args = ["create", "--probe", probe, "--question", "Choose the bucket equal to the status field.",
                "--buckets", ",".join(buckets), "--every", "10s", "--timeout", "150s"]
        for bucket in buckets:
            args += ["--route", f"{bucket}={action}"]
            if matches:
                args += ["--match", f'{bucket}="status": "{bucket}"']
        watch = identifier(self.command(*args), "id")
        self.watches.append(watch)
        require(any(item.get("id") == watch for item in objects(self.command("list"))),
                "Created Sentinel missing from caller-scoped list")
        job = self.command("show", watch)
        require(all(key in job for key in ("id", "creator_tab_id", "team_id", "workspace_id")),
                "Sentinel show lacks captured raw scope")
        require(job["id"] == watch and job["creator_tab_id"] == self.tab and
                job["workspace_id"] == self.workspace, "Sentinel show has foreign scope")
        self.scopes[watch] = {"sentinel_id": job["id"], "tab": job["creator_tab_id"],
                              "team_id": job["team_id"], "workspace_id": job["workspace_id"]}
        return watch

    def rows(self, watch):
        journal = load_journal(self.data_dir)[self.start_row:]
        return [row for row in journal if row.get("sentinel_id") == watch]

    def observe(self):
        return self.dev.snapshot(self.tab)[0]

    def feed(self):
        before = self.dev.get("/api/supporter/activity?limit=100")["entries"]
        self.ci_set("running")
        watch = self.create(["running"], "stop")
        def completed():
            rows = self.rows(watch)
            return any(row.get("action") == "decision" for row in rows) and \
                any(row.get("action") == "stop" for row in rows)
        self.events.until(completed, "Sentinel verdict and stop action persisted before feed projection")
        after = self.dev.get("/api/supporter/activity?limit=100")["entries"]
        remaining = Counter(json.dumps(row, sort_keys=True) for row in before)
        new = []
        for row in after:
            key = json.dumps(row, sort_keys=True)
            if remaining[key]:
                remaining[key] -= 1
            else:
                new.append(row)
        evidence = {"case": "sentinel", "variant": "activity-feed", "jev": self.options.jev,
                    "instance_proven": True, "persisted_decision": completed(),
                    "journal": self.rows(watch), "new_entries": new,
                    "path": "not_applicable_visibility", "heuristic_arm": [], "jev_arm": []}
        self.dev.transcript.record("sentinel_feed_candidate", evidence)
        judge_sentinel_feed(evidence)
        return evidence

    def receipts(self, watch, expected, baseline):
        if self.options.variant == "git":
            payload = self.dev.dc("notify", "list")
            notices = {item["id"]: item for item in objects(payload) if item.get("id") and
                       watch in str(item.get("title", "")) and item["id"] not in baseline}
            require(all(notice.get("tab_id") == self.tab and
                        notice.get("workspace_id") == self.workspace for notice in notices.values()),
                    "Git Sentinel notification did not retain its creator/workspace provenance")
            return [bucket for notice in notices.values() for bucket in expected
                    if bucket in notice.get("title", "")]
        recipient, _ = self.dev.snapshot(self.options.owner_tab)
        require(recipient.get("agent_session_id") == self.recipient_session,
                "Sentinel recipient's own bound session changed")
        payload = self.dev.dc("tab", "chat", "--tab", self.options.owner_tab, "--since", baseline)
        return [bucket for item in payload.get("messages", []) if item.get("type") in ("user", "system")
                for bucket in expected if watch in item.get("content", "") and
                bucket in item.get("content", "")]

    def unchanged(self, watch, status):
        initial = self.signals.count(status)
        claims = sum(row.get("action") == "route_claim" for row in self.rows(watch))
        def completed():
            if self.signals.count(status) < initial + 2:
                return False
            job = self.command("show", watch)
            return job.get("last_reason") == "unchanged"
        self.events.until(completed, "two identical probe process signals and persisted unchanged state")
        require(sum(row.get("action") == "route_claim" for row in self.rows(watch)) == claims,
                "Unchanged output produced a duplicate route")

    def run(self):
        require(self.options.owner_tab, "Sentinel needs the ready named-instance observer")
        owner, _ = self.dev.snapshot(self.options.owner_tab)
        require(ready(owner), "Sentinel recipient has not reached AgentReady and own bound session")
        self.recipient_session = owner["agent_session_id"]
        owner_meta = json.loads((self.root / "owner.json").read_text())
        require(owner_meta["tab_id"] == self.options.owner_tab, "Sentinel observer identity differs")
        self.workspace = owner_meta["workspace_id"]
        created = self.dev.dc("--as-tab", self.options.owner_tab, "tab", "new",
                              "--workspace", owner_meta["workspace_id"], "--cwd", str(self.directory),
                              "--title", "Matrixwatch", "--eager")
        self.tab = identifier(created, "tab_id")
        self.signals = ProbeSignals(self.dev, self.events)
        self.signals.thread.start()
        variant = self.options.variant
        if variant == "activity-feed":
            return self.feed()
        if variant == "regex-no-jev":
            self.set_config("supporter.adapter", "")
            require(self.dev.dc("supporter", "status").get("connected") is False,
                    "Regex provocation still has a connected adapter")
        status = self.dev.dc("supporter", "status")
        require("calls_per_hour" in status.get("budget", {}), "Budget cap field is absent")
        self.dev.transcript.record("cap_semantics", {"calls_per_hour": status["budget"]["calls_per_hour"],
                                                    "uncapped": status["budget"]["calls_per_hour"] is None})
        if variant == "budget-exhausted":
            if self.options.jev == "off":
                raise Finding("Owner-accepted budget-off N/A must bypass live provocation")
            self.set_config("supporter.calls_per_hour", 1)
            if not self.dev.dc("supporter", "status")["budget"]["reached"]:
                self.ci_set("running")
                primer = self.create(["running"], "stop")
                self.events.until(lambda: self.dev.dc("supporter", "status")["budget"]["reached"],
                                  "first real request exhausts the explicit isolated cap")
                self.command("stop", primer)
        baseline = ({item["id"] for item in objects(self.dev.dc("notify", "list")) if item.get("id")}
                    if variant == "git" else self.dev.dc("tab", "chat", "--tab", self.options.owner_tab)["count"])
        repo = self.git_setup() if variant == "git" else None
        if not repo:
            self.ci_set("running")
        expected = ["landed"] if repo else ["running", "success", "failure"]
        action = "notify" if repo else f"send:{self.options.owner_tab}"
        self.model_start_row = len(load_journal(self.data_dir))
        watch = self.create(expected, action)
        def routed(bucket):
            rows = self.rows(watch)
            if any(row.get("reason") == "route_failed" for row in rows):
                raise Finding("Sentinel route failed before the recipient")
            return self.command("show", watch).get("machine", {}).get("last_bucket") == bucket and \
                self.receipts(watch, expected, baseline) == expected[:expected.index(bucket) + 1]
        if repo:
            self.events.until(lambda: self.command("show", watch).get("last_reason") == "other",
                              "git target has not landed and no route was selected")
            self.fixture_command("git", "-C", str(repo), "branch", "-f", "watched", "HEAD")
        if variant == "budget-exhausted":
            self.events.until(lambda: any(row.get("reason") == "budget_exhausted" for row in self.rows(watch)),
                              "changed observation explicitly refuses exhausted model budget")
            self.unchanged(watch, "running")
            evidence = self.evidence(watch, [], baseline)
            self.command("stop", watch)
            self.set_config("supporter.adapter", "")
            deterministic = self.create(["running"], action)
            self.events.until(lambda: self.command("show", deterministic).get("machine", {}).get("last_bucket") == "running"
                              and self.receipts(deterministic, ["running"], baseline) == ["running"],
                              "regex watch still delivers with exhausted global budget")
            self.unchanged(deterministic, "running")
            evidence["deterministic_preserved"] = True
            evidence["deterministic_journal"] = self.rows(deterministic)
        else:
            for generation, bucket in enumerate(expected, 1):
                if not repo and generation > 1:
                    self.ci_set(bucket)
                self.events.until(lambda: routed(bucket), f"{bucket} reached its intended recipient exactly once")
                self.unchanged(watch, bucket)
            evidence = self.evidence(watch, expected, baseline)
        judge_sentinel_outcome(evidence)
        return evidence

    def evidence(self, watch, expected, baseline):
        status = self.dev.dc("supporter", "status")
        journal = load_journal(self.data_dir)[self.model_start_row:]
        calls = [row for row in journal if row.get("tab") == self.tab and
                 row.get("kind") == "model_call" and row.get("verdict_kind") == "sentinel"]
        evidence = {"case": "sentinel", "variant": self.options.variant, "jev": self.options.jev,
                    "instance_proven": True, "sentinel_id": watch, "creator": self.tab,
                    "journal": self.rows(watch), "raw_scope": self.scopes[watch], "expected_buckets": expected,
                    "route_action": "notify" if self.options.variant == "git" else "send",
                    "receipts": self.receipts(watch, expected or ["running", "success", "failure"], baseline),
                    "probe_barrier": True,
                    "unchanged_actions": 0, "model_calls": len(calls), "model_call_journal": calls,
                    "regex_mode": self.options.jev == "off" or self.options.variant == "regex-no-jev",
                    "budget_reached": status["budget"]["reached"], "path": None}
        self.dev.transcript.record("sentinel_outcome", evidence)
        return evidence

    def close(self):
        self.dev.deadline = time.monotonic() + 35
        failures = []
        for watch in self.watches:
            try:
                self.command("stop", watch)
            except Exception as error:
                failures.append(str(error))
        for key, value in reversed(list(self.restore.items())):
            try:
                self.dev.dc("config", "set", key, json.dumps(value))
            except Exception as error:
                failures.append(str(error))
        if self.signals:
            self.signals.close()
        if self.tab:
            try:
                self.dev.dc("tab", "close", "--tab", self.tab)
            except Exception as error:
                failures.append(str(error))
        self.dev.transcript.record("cleanup", {"failures": failures})
        require(not failures, "Sentinel cleanup failed: " + "; ".join(failures))
