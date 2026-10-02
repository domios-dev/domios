"""Event-driven Courier source, interlocked before any provider creation."""

import json
from pathlib import Path
import shlex
import sys
import uuid

from attribution import binding_ready
from busy_fixture import BusyGate
from coordinator_oracles import judge_coordinator
from coordinator_receipts import Phases, at, version_receipt
from fixtures import HERE, case_fixture
from native_assembly import require_native
from oracles import load_journal, require
from scenarios import Scenario
from transport import Pending

INDEPENDENT = ("real_prompt", "isolated_update_scope", "safe_receipts", "prompt_occurrences",
               "safe_choice", "restart_launches", "failure_barrier", "tree_stopped",
               "busy_checkpoint", "idle_receipt", "install_n", "busy_restart_n",
               "decision_rows", "decision_identity", "path")


def preflight(contracts, options, root):
    for name in ("isolation", "update_fixture", "coordinator"):
        if contracts[name]["status"] != "bound":
            raise Pending(contracts[name]["reason"])
    if not binding_ready(contracts["decision"]):
        raise Pending("Courier exact action-envelope/Auditor join remains absent")
    courier = contracts["coordinator"]
    for key in ("authentic_trigger_status", "updater_confinement_status", "termination_status"):
        if courier.get(key) != "bound":
            raise Pending(f"Courier {key} remains pending")
    fields = courier.get("receipt_paths", {})
    if any(not isinstance(fields.get(key), str) or not fields[key] for key in INDEPENDENT):
        raise Pending("Courier independent occurrence/action/idle/tree/barrier receipt selectors remain unbound")
    if any(not courier.get(key) for key in ("session_field", "launch_field", "run_path")):
        raise Pending("Courier exact own-session/launch/persisted run schema remains unbound")
    pins = json.loads((HERE / "versions.json").read_text())["providers"][options.provider]
    require(options.provider == "codex" and options.older_version == pins["older"],
            "Courier requires the reviewed older Codex literal")
    require_native(root, "codex", pins["older"])
    require_native(root, "codex", pins["target"])
    return courier


class CoordinatorScenario(Scenario):
    def __init__(self, dev, events, root, fixture, options, data_dir, contracts):
        super().__init__(dev, events, root, fixture, options)
        self.contract = preflight(contracts, options, root)
        self.contracts, self.data_dir = contracts, data_dir
        self.children = []
        self.busy_gate = None
        self.phases = Phases(options.variant)
        self.start_row = len(load_journal(data_dir))
        self.target = json.loads((HERE / "versions.json").read_text())["providers"]["codex"]["target"]
        self.version_requests = {}
        self.original_sessions = {}

    def capture(self):
        snapshots, chats, launches = {}, {}, []
        for scenario in [self, *self.children]:
            if scenario.tab:
                before = scenario.observe()
                chats[scenario.tab] = self.dev.dc("tab", "chat", "--tab", scenario.tab, "--since", "0")
                after = scenario.observe()
                require(at(before, self.contract["session_field"]) == at(after, self.contract["session_field"]) and
                        at(before, self.contract["launch_field"]) == at(after, self.contract["launch_field"]),
                        "Own session changed across transcript receipt read")
                snapshots[scenario.tab] = after
                launches.extend(scenario.launches())
        state_file = self.root / "daemon-home/.codeterm/provider-update-state.json"
        state = json.loads(state_file.read_text()) if state_file.is_file() else {}
        journal = [row for row in load_journal(self.data_dir)[self.start_row:]
                   if row.get("provider") == "codex"]
        raw = {"snapshots": snapshots, "chats": chats, "launches": launches,
               "state": state, "journal": journal, "notices": self.dev.dc("notify", "list")}
        self.dev.transcript.record("courier_raw", raw)
        return raw

    def project(self, raw):
        # Selectors stay null until actual producers and independent joins are reviewed.
        evidence = {key: at(raw, self.contract["receipt_paths"][key]) for key in INDEPENDENT}
        tabs = [self.tab, *(child.tab for child in self.children)]
        sessions = self.original_sessions or {tab: at(raw["snapshots"][tab], self.contract["session_field"]) for tab in tabs}
        evidence.update(instance_proven=True, provider="codex", target_version=self.target,
                        variant=self.options.variant, jev=self.options.jev, journal=raw["journal"],
                        tabs=tabs, prompt_tabs=[self.tab], sessions=sessions,
                        persisted_run=at(raw["state"], self.contract["run_path"]),
                        owner_notices=[], version_receipts=[])
        summary = evidence["persisted_run"].get("summary_id")
        from transport import objects
        evidence["owner_notices"] = [row for row in objects(raw["notices"]) if row.get("id") == summary]
        if self.options.variant == "busy":
            evidence["busy_tab"] = self.children[0].tab
        return evidence

    def checkpoint(self, phase, predicate):
        def observed():
            raw = self.capture()
            evidence = self.project(raw)
            if predicate(evidence):
                sequence = self.dev.transcript.record("courier_phase", {"phase": phase, "evidence": evidence})
                self.phases.advance(phase, sequence)
                return raw, evidence
            return False
        return self.events.until(observed, f"Courier independent {phase} receipt")

    def start_busy(self):
        from types import SimpleNamespace
        fixture = case_fixture(self.root, "codex", "busy-worker", "busy", "")
        fixture["version"] = self.fixture["version"]
        options = SimpleNamespace(**vars(self.options))
        options.case = "busy-worker"
        child = Scenario(self.dev, self.events, self.root, fixture, options)
        self.children.append(child)
        self.busy_gate = BusyGate(self.dev, self.events)
        child.start()
        child.wait_ready()
        argv = [sys.executable, str(HERE / "busy_fixture.py"), "--port", str(self.busy_gate.port),
                "--nonce", self.busy_gate.nonce, "--deadline", str(self.dev.deadline)]
        import subprocess, os
        command = subprocess.list2cmdline(argv) if os.name == "nt" else shlex.join(argv)
        task = Path(fixture["case_root"]) / "busy-task.txt"
        task.write_text(f"Run this one command using your command tool: {command}\n"
                        "Keep the tool in the foreground. Leave any permission request unanswered.")
        self.dev.dc("send", "--tab", child.tab, "--require-agent", "--from-file", task)
        self.events.until(lambda: self.busy_gate.connected.is_set(), "real busy tool socket start")
        sequence = self.dev.transcript.record("busy_started", self.busy_gate.receipt)
        self.phases.advance("busy_started", sequence)

    def request_version(self, tab):
        nonce = uuid.uuid4().hex
        argv = [sys.executable, str(HERE / "version_probe.py"), "--nonce", nonce]
        import subprocess, os
        command = subprocess.list2cmdline(argv) if os.name == "nt" else shlex.join(argv)
        task = Path(self.fixture["case_root"]) / f"version-{tab}.txt"
        task.write_text(f"Use your command tool once to run: {command}\n"
                        "This reads codex --version from this tab's own environment. "
                        "Leave any permission request unanswered. Do not infer a version or run another command.")
        self.dev.dc("send", "--tab", tab, "--require-agent", "--from-file", task)
        self.version_requests[tab] = (command, nonce)

    def run(self):
        enabled = self.dev.dc("config", "get", "updater.allow_provider_updates_in_non_prod")
        if enabled.get("value") is not True:
            raise Pending("Nonprod updater remains disabled; no harness opt-in is authorized")
        if self.options.variant == "busy":
            self.start_busy()
        self.start()
        _, prompt = self.checkpoint("prompt", lambda value: value["real_prompt"] is True)
        self.original_sessions = prompt["sessions"].copy()
        failed = self.options.variant == "fail"
        label = "update_failed" if failed else "update_succeeded"
        phase = "failed" if failed else "installed_busy" if self.options.variant == "busy" else "installed"
        self.checkpoint(phase, lambda value: any(row.get("label") == label for row in value["journal"]) and
                        (self.options.variant != "busy" or value["busy_checkpoint"].get("active_turn") is True))
        if failed:
            self.checkpoint("tree_stopped", lambda value: value["tree_stopped"] is True)
            _, evidence = self.checkpoint("failure_barrier", lambda value: value["failure_barrier"] is True)
        else:
            if self.busy_gate:
                self.busy_gate.release()
                sequence = self.dev.transcript.record("released", {"nonce": self.busy_gate.nonce})
                self.phases.advance("released", sequence)
                self.checkpoint("idle", lambda value: value["idle_receipt"].get("active_turn") is False and
                                value["idle_receipt"].get("active_attempt") is False)
            raw, evidence = self.checkpoint("resumed", lambda value: len(value["restart_launches"]) == len(value["tabs"]))
            for tab in evidence["tabs"]:
                self.request_version(tab)
            def versions():
                raw = self.capture()
                current = self.project(raw)
                receipts = []
                for tab, (executable, nonce) in self.version_requests.items():
                    expected = {"tab": tab, "session": at(raw["snapshots"][tab], self.contract["session_field"]), "version": self.target,
                                "launch": at(raw["snapshots"][tab], self.contract["launch_field"])}
                    try:
                        receipts.append(version_receipt(raw["chats"][tab].get("messages", []), expected, executable, nonce))
                    except Exception:
                        return False
                current["version_receipts"] = receipts
                judge_coordinator(current)
                return current
            evidence = self.events.until(versions, "all own-session target version tool results")
        judge_coordinator(evidence)
        self.phases.advance("verified", self.dev.transcript.record("courier_verified", evidence))
        return evidence

    def close(self):
        failures = []
        if self.busy_gate:
            try:
                self.busy_gate.close()
            except Exception as error:
                failures.append(str(error))
        for child in self.children:
            try:
                child.close()
            except Exception as error:
                failures.append(str(error))
            from launch_env import remove_scope
            remove_scope(self.root, child.fixture["scope_id"])
        try:
            super().close()
        except Exception as error:
            failures.append(str(error))
        require(not failures, "Courier owned cleanup failed: " + "; ".join(failures))
