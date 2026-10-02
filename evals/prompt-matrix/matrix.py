"""WHAT: Own bounded live gates, their cleanup and per-cell transcripts.
WHY: Source-only authoring must leave canonical QA exact executable commands.
ORACLE: A PASS requires independently checked real prompt and policy evidence.
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import sys
import time
import urllib.request

from fixtures import (HERE, REPO, PROVIDERS, canonical, case_fixture, environment,
                      fixture_root, prepare, write_json)
from oracles import (decision_path, judge, load_journal, objects, prompt_kind,
                     ready, require, tab_notifications)
from scenarios import CASES, Scenario, identifier, install_older, pending_special
from sentinel_oracles import judge_sentinel
from sentinel_scenarios import SentinelScenario
from transport import Dev, Events, Finding, InstanceLock, Pending, Transcript, read_secret, run_group
from attribution import binding_ready
from launch_env import register_scope, remove_scope
from native_assembly import require_native
from coordinator_scenarios import CoordinatorScenario, preflight as courier_preflight


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--instance", required=True)
    parser.add_argument("--port", required=True, type=int)
    mode = parser.add_mutually_exclusive_group()
    for name in ("prepare", "start", "stop", "smoke"):
        mode.add_argument("--" + name, action="store_true")
    mode.add_argument("--connect-jev", action="store_true")
    mode.add_argument("--disconnect-jev", action="store_true")
    mode.add_argument("--create-owner", action="store_true")
    mode.add_argument("--remove-owner", action="store_true")
    parser.add_argument("--provider", choices=PROVIDERS)
    parser.add_argument("--case", choices=(*CASES, "update-coordinator", "sentinel"))
    parser.add_argument("--jev", choices=("off", "on"), default="off")
    parser.add_argument("--variant", choices=("default", "busy", "fail", "git", "fake-ci",
                                             "regex-no-jev", "budget-exhausted", "activity-feed", "disagreement"),
                        default="default")
    parser.add_argument("--older-version")
    parser.add_argument("--owner-tab")
    parser.add_argument("--decision-point", default="")
    parser.add_argument("--bad-model", default="prompt-matrix-does-not-exist-124")
    parser.add_argument("--internal", action="store_true", help=argparse.SUPPRESS)
    return parser.parse_args()


def status_connected(status):
    require(isinstance(status.get("connected"), bool),
            "Supporter status must expose its authoritative connected predicate")
    return status["connected"]


def identity(dev, options, root):
    value = dev.dc("identify")
    connection = value.get("connection", {})
    require(connection.get("instance") == options.instance, "Named instance differs")
    require(connection.get("host") in ("127.0.0.1", "localhost"), "Non-loopback instance refused")
    require(connection.get("port") == options.port, "Named instance port differs")
    data_dir = Path(connection.get("data_dir", "")).resolve()
    require(data_dir == (root / "daemon-home" / f".codeterm-dev-{options.instance}").resolve(),
            "Named instance data escaped its isolated daemon home")
    dev.transcript.record("identity", value)
    request = urllib.request.Request(f"http://127.0.0.1:{options.port}/api/health",
                                    headers={"Authorization": "Bearer " + dev.env["CODETERM_API_TOKEN"]})
    with urllib.request.urlopen(request, timeout=min(5, dev.remaining())) as response:
        health = json.load(response)
    dev.transcript.record("health", health)
    code, head, error = run_group(["git", "rev-parse", "HEAD"], dev.env, min(5, dev.remaining()))
    require(code == 0 and health.get("build_sha") == head.strip(),
            f"Running build SHA differs from canonical HEAD: {health.get('build_sha')} {error}")
    return data_dir


def evidence_for(dev, scenario, before_ids, journal_start, data_dir, baseline_launches):
    tab = scenario.observe()
    all_notifications = dev.dc("notify", "list")
    notices = tab_notifications(all_notifications, scenario.tab, before_ids)
    journal = load_journal(data_dir)[journal_start:]
    dev.transcript.record("journal", journal)
    rows = [row for row in journal if row.get("tab") == scenario.tab or
            row.get("tab_id") == scenario.tab]
    point_rows = [row for row in rows if row.get(scenario.fixture["point_field"]) ==
                  scenario.fixture["decision_point"]]
    case = scenario.options.case
    state = prompt_kind(tab)
    expected_states = {
        "permission": {"permission"},
        "login": {"login", "login_or_auth_required", "onboarding", "provider"},
        "bad-model": {"model_unavailable", "error"},
        "update": {"update", "update_prompt", "provider_update_prompt"},
    }
    frames = [row["value"] for row in read_transcript(dev.transcript.directory)
              if row["kind"] == "event"]
    observed = any(
        item.get("tab_id") == scenario.tab and item.get("prompt")
        for frame in frames for item in objects(frame.get("data", {}))
    )
    trust = case.startswith("trust-")
    trust_answer = any(row.get("reason") == "deterministic_directory_trust" for row in rows)
    observations = [row for row in rows if row.get("action") == "verdict"]
    completed = [row for row in point_rows if row.get("decided_by") and row.get("observation_id") is not None]
    observation_ids = {str(row["observation_id"]) for row in completed}
    if case == "permission" and tab.get("pending_prompt") and state == "permission":
        current_identity = json.dumps(tab["pending_prompt"], sort_keys=True)
        scenario.fixture.setdefault("permission_identity", current_identity)
    if case == "permission" and scenario.fixture.get("permission_identity"):
        require(json.dumps(tab.get("pending_prompt"), sort_keys=True) ==
                scenario.fixture["permission_identity"], "Permission request was answered, withdrawn or replaced")
    answers = [item for item in objects(scenario.last_payload)
               if item.get("transition") == "permission_auto_confirm" and
               item.get("decision") == "confirmed"]
    answered_notices = [item for item in notices if item.get("answered")]
    require(not (case == "permission" and (answers or answered_notices)),
            "Permission answer recorded while default policy forbids it")
    expected = "answer" if case in ("trust-hidden", "trust-cron", "trust-visible", "update") else "notify"
    launches = scenario.launches()
    own_launches = [row for row in launches if row["tab_id"] == scenario.tab]
    route_proven = bool(notices) and all(
        notice.get("target_tab_id") == scenario.options.owner_tab
        if scenario.options.owner_tab else notice.get("level") in ("action_needed", "failure")
        for notice in notices
    )
    evidence = {
        "case": case, "provider": scenario.options.provider, "jev": scenario.options.jev,
        "variant": scenario.options.variant,
        "expected": expected, "instance_proven": True, "real_provider": bool(own_launches),
        "prompt_observed": observed and (trust or state in expected_states.get(case, set())),
        "path": decision_path(point_rows, scenario.tab, scenario.options.jev),
        "precondition_n": scenario.precondition_n, "outcome_n": dev.transcript.sequence,
        "notifications": len(notices), "route_proven": route_proven,
        "dedup_barrier": len(observation_ids) >= 2,
        "answer_committed": trust_answer or any(row.get("answer_committed") is True for row in rows),
        "far_end": ready(tab), "answers": len(answers) + len(answered_notices),
        "pending_same": bool(tab.get("pending_prompt")) and state == "permission" and
                        json.dumps(tab.get("pending_prompt"), sort_keys=True) ==
                        scenario.fixture.get("permission_identity"),
        "observation_barrier": len(observation_ids) >= 2,
        "tool_completed": Path(scenario.fixture["permission_target"]).exists()
                          if "permission_target" in scenario.fixture else None,
        "own_pid_signalled": scenario.fixture.get("own_pid_signalled", False),
        "restart_count": max(0, len(own_launches) - baseline_launches),
        "restart_limit": 0,
        "heuristic_arm": [row for row in rows if row.get("decided_by") == "heuristic" or row.get("heuristic_verdict")],
        "jev_arm": observations + [row for row in completed if str(row.get("decided_by", "")).startswith("fallback(")],
        "fallback_reason": [row.get("reason") for row in rows if row.get("kind") == "fallback"],
        "disagreement": [row for row in rows if row.get("disagreement") is True],
    }
    if case == "crash":
        evidence["prompt_observed"] = scenario.fixture.get("own_pid_signalled", False) and (
            tab.get("exit_suspected_at") is not None or not tab.get("agent_type") or
            len(own_launches) > baseline_launches)
    return evidence


def read_transcript(directory):
    path = directory / "transcript.jsonl"
    return [json.loads(line) for line in path.read_text().splitlines() if line]


def create_owner(dev, root, provider):
    require(provider, "--create-owner requires --provider")
    require(not (root / "owner.json").exists(), "Matrix owner already exists")
    pins = json.loads((HERE / "versions.json").read_text())["providers"].get(provider, {})
    if provider == "cursor":
        raise Pending("Cursor fixture-native executable remains unbound")
    require_native(root, provider, pins["target"])
    fixture = case_fixture(root, provider, "observer", "default", "")
    workspace = dev.dc("workspace", "create", "Matrixowner", "--cwd", fixture["cwd"])
    register_scope(root, fixture, Path(fixture["cwd"]))
    ws = identifier(workspace, "id")
    task = root / "owner-task.txt"
    task.write_text(
        "You observe a prompt-policy test. Reply READY once, then remain available for reports. "
        "Never answer provider prompts, restart agents, change settings, edit files or call tools. "
        "For every received test event, reply RECEIVED once in this conversation."
    )
    tab = None
    try:
        with Events(dev) as events:
            created = dev.dc("agent", "spawn", provider, "--title", "Matrixowner",
                             "--workspace", ws, "--task-file", str(task))
            tab = identifier(created, "tab_id")
            events.until(lambda: ready(dev.snapshot(tab)[0]), "Matrix owner AgentReady and own bound session")
        write_json(root / "owner.json", {"tab_id": tab, "workspace_id": ws, "fixture": fixture})
    except BaseException:
        dev.deadline = time.monotonic() + 30
        dev.dc("workspace", "close", ws)
        remove_scope(root, fixture["scope_id"])
        raise


def live(options, root):
    case_key = f"{options.provider or 'host'}-{options.case or 'smoke'}-{options.jev}-{options.variant}"
    run_id = str(time.time_ns())
    transcript = Transcript(root / "transcripts" / case_key / run_id)
    dev = Dev(dict(os.environ), transcript)
    result = {"provider": options.provider or "host", "case": options.case or "smoke",
              "jev": options.jev, "variant": options.variant, "status": "FAIL",
              "decision_point": options.decision_point,
              "run_id": run_id, "transcript": str(transcript.directory), "path": None}
    scenario = None
    fixture = None
    try:
        data_dir = identity(dev, options, root)
        contracts = json.loads((HERE / "contracts.json").read_text())
        if options.create_owner:
            if contracts["isolation"]["status"] != "bound":
                raise Pending(contracts["isolation"]["reason"])
            create_owner(dev, root, options.provider)
            result.update(status="SMOKE", reason="Matrix owner reached AgentReady with its own bound session")
            return 0
        if options.remove_owner:
            owner = json.loads((root / "owner.json").read_text())
            dev.dc("workspace", "close", owner["workspace_id"])
            remove_scope(root, owner["fixture"]["scope_id"])
            shutil.rmtree(owner["fixture"]["case_root"])
            (root / "owner.json").unlink()
            result.update(status="SMOKE", reason="Owned observer workspace closed")
            return 0
        if options.connect_jev:
            dev.dc("supporter", "use", "typesafe-jev")
            dev.dc("supporter", "token", "set", "--adapter", "typesafe-jev", stdin=read_secret())
            require(status_connected(dev.dc("supporter", "status")), "Jev did not connect")
            result.update(status="SMOKE", reason="Jev token accepted on stdin; connected predicate proven")
            return 0
        if options.disconnect_jev:
            dev.dc("supporter", "token", "clear", "--adapter", "typesafe-jev")
            require(not status_connected(dev.dc("supporter", "status")), "Jev remains connected")
            result.update(status="SMOKE", reason="Jev disconnected on isolated instance")
            return 0
        if options.smoke:
            for command in (("workspace", "list"), ("agent", "providers"),
                            ("notify", "list"), ("supporter", "status")):
                dev.dc(*command)
            for command in (("agent", "spawn"), ("tab", "capture", "start"),
                            ("tab", "inspect"), ("cron", "trigger"), ("mem", "worker", "spawn"),
                            ("sentinel", "create"), ("sentinel", "list"),
                            ("sentinel", "show"), ("sentinel", "stop")):
                dev.dc(*command, "--help", json_mode=False)
            result.update(status="SMOKE", reason="Real named-instance CLI read/help calls completed")
            return 0
        require(options.case, "--case is required for a live cell")
        if options.case == "sentinel":
            accepted = next((cell for cell in contracts["sentinel"]["not_applicable"]
                             if cell["variant"] == options.variant and cell["jev"] == options.jev), None)
            if accepted:
                result.update(status="N/A", reason=accepted["reason"], authority=accepted["authority"],
                              path="not_applicable_disconnected_regex")
                return 0
        status = dev.dc("supporter", "status")
        require(status_connected(status) == (options.jev == "on"),
                "Connected Jev precondition differs from requested arm")
        if options.jev == "on":
            require(status.get("adapter") == "typesafe-jev", "Requested arm is not connected Jev")
        if contracts["isolation"]["status"] != "bound":
            raise Pending(contracts["isolation"]["reason"])
        pending_special(options)
        if options.case == "update-coordinator":
            courier_preflight(contracts, options, root)
        if options.case == "sentinel" and options.variant == "activity-feed":
            require(options.jev == "on", "Feed regression has one connected-Jev cell")
            fixture = case_fixture(root, "codex", options.case, options.variant, options.bad_model)
            with Events(dev) as events:
                scenario = SentinelScenario(dev, events, root, fixture, options, data_dir)
                evidence = scenario.run()
                result.update(status="PASS", evidence=evidence, path=evidence["path"],
                              reason="Persisted Sentinel verdict appeared in the owner activity API")
            return 0
        decision_contract = contracts["decision"]
        if not binding_ready(decision_contract):
            raise Pending(decision_contract["reason"])
        point_field = decision_contract.get("point_field")
        point = decision_contract.get("case_points", {}).get(options.case)
        if not point_field or not point:
            raise Pending("Final per-decision-point journal correlation is not bound for this case")
        if options.case == "sentinel":
            sentinel_contract = json.loads((HERE / "contracts.json").read_text())["sentinel"]
            require(sentinel_contract["cli_status"] == "bound", "Sentinel CLI contract is not bound")
            require(options.variant in sentinel_contract["variants"], "Sentinel variant is not bound")
            fixture = case_fixture(root, "codex", options.case, options.variant, options.bad_model)
            with Events(dev) as events:
                scenario = SentinelScenario(dev, events, root, fixture, options, data_dir)
                evidence = scenario.run()
                rows = [row for row in evidence["journal"] if row.get(point_field) == point]
                evidence.update(
                    path=decision_path(rows, scenario.tab, options.jev),
                    heuristic_arm=[row for row in rows if row.get("heuristic_verdict") or
                                   row.get("decided_by") == "heuristic"],
                    jev_arm=[row for row in rows if row.get("action") == "verdict" or
                             str(row.get("decided_by", "")).startswith("fallback(")],
                    observation_ids=[row["observation_id"] for row in rows if row.get("observation_id") is not None],
                )
                if evidence["path"] is None:
                    result.update(evidence=evidence)
                    raise Pending("Sentinel outcomes captured; final correlated Auditor path is absent")
                judge_sentinel(evidence)
                result.update(status="PASS", evidence=evidence, path=evidence["path"], reason="Bound Sentinel oracles passed")
            return 0
        require(options.provider, "--provider is required")
        if options.provider == "cursor":
            raise Pending("Cursor fixture-native availability remains pending; owner executable is forbidden")
        version = json.loads((HERE / "versions.json").read_text())["providers"][options.provider]["target"]
        if options.case in ("update", "update-coordinator"):
            version = install_older(dev, root, options.older_version, options.provider)
        require_native(root, options.provider, version)
        fixture = case_fixture(root, options.provider, options.case, options.variant, options.bad_model)
        fixture["point_field"] = point_field
        fixture["decision_point"] = point
        fixture["version"] = version
        baseline = dev.dc("notify", "list")
        before_ids = {item["id"] for item in objects(baseline) if item.get("id")}
        journal_start = len(load_journal(data_dir))
        with Events(dev) as events:
            if options.case == "update-coordinator":
                scenario = CoordinatorScenario(dev, events, root, fixture, options, data_dir, contracts)
                evidence = scenario.run()
                result.update(status="PASS", evidence=evidence, path=evidence["path"],
                              reason="Independent Courier outcomes and attribution verified")
                return 0
            scenario = Scenario(dev, events, root, fixture, options)
            scenario.start()
            launches = len(scenario.launches())
            def policy_completed():
                evidence = evidence_for(dev, scenario, before_ids, journal_start, data_dir, launches)
                transcript.record("oracle_candidate", evidence)
                if evidence["tool_completed"] is True and options.case == "permission":
                    raise Finding("Permission tool executed without an owner answer")
                if not evidence["prompt_observed"]:
                    return False
                if evidence["expected"] == "answer" and not evidence["far_end"]:
                    return False
                if evidence["expected"] == "notify" and evidence["notifications"] == 0:
                    return False
                if evidence["path"] is None or not evidence["dedup_barrier"]:
                    return False
                judge(evidence)
                return evidence
            evidence = events.until(policy_completed, "real prompt -> policy decision -> far-end oracle")
            result.update(status="PASS", evidence=evidence, path=evidence["path"], reason="All bound oracles passed")
        return 0
    except Pending as error:
        result.update(status="PENDING", reason=str(error))
        return 3
    except Exception as error:
        result.update(status="FAIL", reason=str(error))
        transcript.record("finding", str(error))
        (transcript.directory / "FINDING.md").write_text(
            f"# Prompt matrix finding\n\nCell: {case_key}\n\nOracle: {error}\n\n"
            "No repair was made in the tester lane. See transcript.jsonl for exact commands, screens, "
            "notifications, journal and cleanup. Report this finding to the owning Orchestrator.\n"
        )
        if scenario and scenario.tab:
            try:
                scenario.observe()
                transcript.record("final_notifications", dev.dc("notify", "list"))
                transcript.record("final_journal", load_journal(data_dir))
            except Exception as capture_error:
                transcript.record("capture_failure", str(capture_error))
        return 1
    finally:
        if scenario:
            try:
                scenario.close()
            except Exception as error:
                result.update(status="FAIL", cleanup_failure=str(error))
        if fixture:
            transcript.record("provider_launches", scenario.launches() if isinstance(scenario, Scenario) else [])
            shutil.rmtree(fixture["case_root"])
            remove_scope(root, fixture["scope_id"])
        write_json(transcript.directory / "result.json", result)
        print(json.dumps(result, indent=2))
        if result.get("cleanup_failure"):
            raise Finding(result["cleanup_failure"])


def main():
    options = arguments()
    canonical()
    root = fixture_root(options.instance, options.port)
    if options.prepare:
        print(prepare(options.instance, options.port))
        return 0
    require((root / "instance.json").is_file(), "Run --prepare in canonical first")
    meta = json.loads((root / "instance.json").read_text())
    require(meta["instance"] == options.instance and meta["port"] == options.port,
            "Prepared fixture identity differs from requested gate")
    env = environment(root)
    if options.owner_tab is None and (root / "owner.json").exists():
        options.owner_tab = json.loads((root / "owner.json").read_text())["tab_id"]
    if options.start or options.stop:
        if options.start:
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", options.port))
        command = ["bash", str(REPO / "build.sh"), "dev", "--wait"] if options.start else [
            "bash", str(REPO / "build.sh"), "kill"]
        # build.sh owns the machine gateway and operation-specific deadlines.
        os.execvpe("bash", command, env)
    if options.internal:
        return live(options, root)
    command = ["bash", "-c", 'source "$1"; shift; dev_run "$@"', "matrix-dev",
               str(REPO / "evals/_lib/lib.sh"), sys.executable, str(HERE / "matrix.py"),
               *sys.argv[1:], "--internal"]
    secret = read_secret() if options.connect_jev else None
    with InstanceLock(root):
        code, out, err = run_group(command, env, 240, secret)
    print(out, end="")
    print(err, end="", file=sys.stderr)
    return code


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(f"prompt-matrix gate refused: {error}", file=sys.stderr)
        sys.exit(7)
