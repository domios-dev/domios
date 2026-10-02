"""WHAT: Provoke real provider states without answering their prompts.
WHY: Permission safety must be exercised at the provider, not a canned screen.
ORACLE: Each provoker captures a real launch/state before policy observation.
"""

import json
import os
from pathlib import Path
import signal

from fixtures import HERE, REPO, write_json
from launch_env import register_scope
from oracles import ready, require
from transport import Finding, Pending, objects, run_group


ROLE = "Read no files and call no tools. Reply with READY once, then wait for instructions."
CASES = ("trust-hidden", "trust-cron", "trust-visible", "trust-lane", "update",
         "login", "permission", "bad-model", "crash")


def identifier(payload, key):
    matches = {item[key] for item in objects(payload) if isinstance(item.get(key), str)}
    require(len(matches) == 1, f"Expected one unambiguous {key}; received {matches}")
    return matches.pop()


class Scenario:
    def __init__(self, dev, events, root, fixture, options):
        self.dev, self.events, self.root = dev, events, root
        self.fixture, self.options = fixture, options
        self.workspace = None
        self.tab = None
        self.cron = None
        self.worker = None
        self.capture = False
        self.precondition_n = 0

    def observe(self):
        tab, payload = self.dev.snapshot(self.tab)
        self.last_payload = payload
        return tab

    def workspace_for_case(self):
        case = self.options.case
        if case in ("trust-cron", "trust-lane"):
            cwd = Path(self.fixture["cwd"])
            if case == "trust-lane":
                self.make_git_lane(cwd)
                cwd = cwd.parent / "lane"
            payload = self.dev.dc("workspace", "create", "Matrix", "--cwd", cwd)
        else:
            payload = self.dev.dc("workspace", "create", "Matrix", "--provision")
        self.workspace = identifier(payload, "id")
        cwd = next((item["cwd"] for item in objects(payload) if item.get("cwd")), None)
        require(cwd, "Created workspace has no cwd")
        return cwd

    def make_git_lane(self, cwd):
        env = self.dev.env
        for argv in (
            ["git", "init", str(cwd)],
            ["git", "-C", str(cwd), "-c", "user.name=Matrix", "-c",
             "user.email=matrix@example.invalid", "commit", "--allow-empty", "-m", "fixture"],
            ["git", "-C", str(cwd), "worktree", "add", "-b", "lane/matrix", str(cwd.parent / "lane")],
        ):
            self.dev.transcript.record("fixture_command", argv)
            code, out, err = run_group(argv, env, min(15, self.dev.remaining()))
            require(code == 0, f"Git lane setup failed: {err or out}")

    def start(self):
        case = self.options.case
        provider = self.options.provider
        self.precondition_n = self.dev.transcript.record("provocation", {"provider": provider, "case": case})
        if case == "trust-hidden":
            require(self.options.owner_tab, "Hidden worker needs --owner-tab on the named instance")
            meta = json.loads((self.root / "instance.json").read_text())
            cwd = self.root / "daemon-home" / f".codeterm-dev-{meta['instance']}" / "workers"
            cwd.mkdir(parents=True, exist_ok=True)
            register_scope(self.root, self.fixture, cwd)
            result = self.dev.dc("mem", "worker", "spawn", "--intent", "prompt-matrix",
                                 "--task", ROLE, "--agent", provider,
                                 "--parent", self.options.owner_tab)
            self.worker = identifier(result, "worker_id")
            self.tab = identifier(result, "tab_id")
        else:
            cwd = self.workspace_for_case()
            register_scope(self.root, self.fixture, Path(cwd))
            if case == "trust-cron":
                require(self.options.owner_tab, "Cron needs --owner-tab on the named instance")
                result = self.dev.dc("--as-tab", self.options.owner_tab, "cron", "create", "Matrix",
                                     "--schedule", "0 0 1 1 *", "--task", ROLE, "--agent", provider,
                                     "--cwd", cwd, "--workspace", self.workspace,
                                     "--tab", self.options.owner_tab, "--timeout-secs", "150")
                self.cron = identifier(result, "id")
                self.dev.dc("cron", "trigger", self.cron)
                def fired():
                    history = self.dev.dc("cron", "history", self.cron)
                    ids = [item["tab_id"] for item in objects(history) if item.get("tab_id")]
                    return ids[-1] if ids else None
                self.tab = self.events.until(fired, "cron has launched its exact run tab")
            else:
                arguments = ["agent", "spawn", provider, "--title", "Matrix", "--cwd", cwd,
                             "--workspace", self.workspace]
                if self.options.owner_tab:
                    arguments += ["--parent-tab", self.options.owner_tab]
                if case not in ("login", "update"):
                    task = Path(self.fixture["case_root"]) / "role.txt"
                    task.write_text(ROLE)
                    arguments += ["--task-file", str(task)]
                result = self.dev.dc(*arguments)
                self.tab = identifier(result, "tab_id")
        self.dev.dc("tab", "auto-confirm", "off", "--tab", self.tab)
        self.dev.dc("tab", "capture", "start", "--tab", self.tab,
                    "--out", str(self.dev.transcript.directory / "terminal.cast"))
        self.capture = True
        self.dev.transcript.record("provoked", {"tab_id": self.tab, "case": case})
        if case == "permission":
            self.permission()
        elif case == "crash":
            self.crash()
        return self.tab

    def wait_ready(self):
        return self.events.until(lambda: ready(self.observe()), "AgentReady + own bound session + no prompt")

    def permission(self):
        self.wait_ready()
        outside = Path(self.fixture["case_root"]) / "outside-workspace"
        outside.mkdir()
        target = outside / "permission-proof.txt"
        task = Path(self.fixture["case_root"]) / "permission-task.txt"
        task.write_text(
            f"Use your command tool to create the file {target} containing MATRIX_PERMISSION_PROOF. "
            "Request approval through the provider. Do not change permission settings or retry. "
            "Leave a pending permission request unanswered."
        )
        self.dev.dc("send", "--tab", self.tab, "--require-agent", "--from-file", str(task))
        self.fixture["permission_target"] = str(target)

    def crash(self):
        if os.name == "nt":
            raise Pending("Windows crash provoker needs the own-tab process-tree CLI contract")
        self.wait_ready()
        launches = self.launches()
        exact = [row for row in launches if row["tab_id"] == self.tab]
        require(len(exact) == 1, "Crash needs one live exact provider launch record")
        row = exact[0]
        require(row["pid"] > 1, "Unsafe provider PID")
        argv = ["ps", "-Ao", "pid=,ppid=,comm="]
        code, out, err = run_group(argv, self.dev.env, min(5, self.dev.remaining()))
        require(code == 0 and out.strip(), f"Provider process tree absent: {err}")
        tree = {}
        for line in out.splitlines():
            parts = line.strip().split(maxsplit=2)
            if len(parts) == 3 and parts[0].isdigit() and parts[1].isdigit():
                tree[int(parts[0])] = (int(parts[1]), parts[2])
        require(row["pid"] in tree and tree[row["pid"]][0] == row["ppid"],
                "Provider launch root no longer belongs to its recorded parent")
        descendants = {row["pid"]}
        for _ in range(len(tree)):
            added = {pid for pid, (ppid, _) in tree.items() if ppid in descendants} - descendants
            if not added:
                break
            descendants |= added
        names = {self.options.provider, Path(row["executable"]).name}
        candidates = [pid for pid in descendants if Path(tree[pid][1]).name in names]
        require(len(candidates) == 1, "Own process tree has no unique native provider PID")
        target = candidates[0]
        own_tree = {str(pid): tree[pid] for pid in descendants}
        self.dev.transcript.record("signal", {"tab_id": self.tab, "pid": target,
                                              "signal": "SIGTERM", "own_process_tree": own_tree})
        os.kill(target, signal.SIGTERM)
        self.fixture["own_pid_signalled"] = True

    def launches(self):
        return [json.loads(path.read_text()) for path in
                Path(self.fixture["case_root"]).glob("launch-*.json")]

    def close(self):
        failures = []
        self.dev.deadline = __import__("time").monotonic() + 35
        steps = []
        if self.capture:
            steps.append(("tab", "capture", "stop", "--tab", self.tab))
        if self.worker:
            steps.append(("mem", "worker", "kill", self.worker))
        if self.cron:
            steps += [("cron", "cancel", self.cron), ("cron", "delete", self.cron)]
        if self.tab and not self.worker:
            steps.append(("tab", "close", "--tab", self.tab))
        if self.workspace:
            steps.append(("workspace", "close", self.workspace))
        for step in steps:
            try:
                self.dev.dc(*step)
            except Exception as error:
                failures.append(str(error))
        self.dev.transcript.record("cleanup", {"failures": failures})
        if failures:
            raise Finding("Owned resource cleanup failed: " + "; ".join(failures))


def install_older(dev, root, version, provider="codex"):
    from native_assembly import require_native
    pins = json.loads((HERE / "versions.json").read_text())["providers"].get(provider)
    if not pins or provider == "cursor":
        raise Pending("Cursor portable native/update binding and owner disposition remain pending")
    require(version == pins["older"], "Older version differs from the reviewed literal pin")
    contracts = json.loads((HERE / "contracts.json").read_text())
    if contracts["update_fixture"]["status"] != "bound":
        raise Pending(contracts["update_fixture"]["reason"])
    manifest, executable = require_native(root, provider, version)
    dev.transcript.record("older_fixture", {"version": version, "executable": str(executable),
                                            "sha256": manifest["executable_sha256"]})
    return version


def pending_special(options):
    if options.variant == "disagreement":
        raise Pending("Real disagreement provoker, emitted point/occurrence and paired-arm evidence must bind first")
    if options.case == "update" and options.provider == "cursor":
        raise Pending("Cursor archive exists locally but its manifest declares no updater; owner disposition is pending")
