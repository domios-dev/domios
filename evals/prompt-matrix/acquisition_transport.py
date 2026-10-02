"""Own exactly one isolated registry helper and require its exit and closed handles."""

import hashlib
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import threading
import time

from acquisition_worker import (MAX_RECEIPT, MAX_REQUEST, bounded_json, contained, metadata_input,
                                exclusive_json, exclusive_file, file_digest, registry_url, read_public_ca, public_ca_bytes,
                                stat_identity, validated_tls)
from deadline_io import Deadline
from native_layout import demand

HERE = Path(__file__).resolve().parent
CLEANUP_RESERVE = 2


def fixture_binding(destination):
    destination = Path(destination).absolute()
    base = (HERE.parent.parent / "target-check/evals/prompt-matrix").resolve()
    demand(destination.is_relative_to(base), "Acquisition destination outside named fixture")
    parts = destination.relative_to(base).parts
    demand(len(parts) >= 5 and re.fullmatch(r"[a-z][a-z0-9-]{2,39}", parts[0]), "Named fixture path absent")
    root = base / parts[0]
    contained(root, destination)
    _, identity = bounded_json(contained(root, root / "instance.json"), MAX_REQUEST)
    demand(identity["instance"] == parts[0] and isinstance(identity["port"], int) and
           1024 <= identity["port"] <= 65535 and identity["port"] not in (7685, 7686), "Fixture identity differs")
    demand(parts[1] == "native" and parts[2] in ("claude", "codex", "opencode") and
           re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", parts[3]) and len(parts) == 5 and
           parts[4] in ("wrapper.tgz", "native.tgz"), "Acquisition path is not an exact native fixture archive")
    demand(destination.parent.is_dir() and not destination.exists(), "Exclusive acquisition destination unavailable")
    return root, destination


def reviewed_metadata(metadata, destination):
    selected = metadata_input(metadata)
    _, pins = bounded_json(HERE / "versions.json", 1024 * 1024)
    destination = Path(destination)
    value = pins["providers"][destination.parent.parent.name]["versions"][destination.parent.name]
    candidates = [value] if destination.name == "wrapper.tgz" else list(value["native_distribution_metadata"].values())
    demand(any(metadata_input(value) == selected for value in candidates), "Acquisition metadata is not reviewed")
    return selected


def helper_environment(home, source=None, platform=None):
    source = os.environ if source is None else source
    platform = sys.platform if platform is None else platform
    home = Path(home).absolute()
    environment = {"HOME": str(home), "USERPROFILE": str(home), "APPDATA": str(home),
                   "LOCALAPPDATA": str(home), "TMPDIR": str(home), "TMP": str(home), "TEMP": str(home), "PATH": ""}
    if platform == "win32":
        system_root = source.get("SystemRoot") or source.get("SYSTEMROOT")
        demand(system_root and Path(system_root).is_absolute() and Path(system_root).is_dir(),
               "Windows trusted SystemRoot unavailable")
        environment["SystemRoot"] = str(Path(system_root).resolve())
    return environment


def acquisition_tls(root, workspace, end, clock=time.monotonic, platform=None):
    platform = sys.platform if platform is None else platform
    if platform in ("linux", "win32"):
        return {"platform": platform, "mode": "system_default"}
    demand(platform == "darwin", "Acquisition TLS platform unsupported")
    raw, resolved, identity = read_public_ca(end, clock)
    snapshot = contained(root, workspace / "public-ca.pem")
    with exclusive_file(snapshot) as output:
        demand(output.write(raw) == len(raw), "Public CA snapshot write incomplete")
    snapshot.chmod(0o400)
    tls = {"platform": platform, "mode": "os_public_ca_snapshot", "source": "/etc/ssl/cert.pem",
           "resolved_source": str(resolved), "source_identity": identity, "snapshot": str(snapshot),
           "snapshot_identity": stat_identity(snapshot.stat()), "bytes": len(raw), "sha256": public_ca_bytes(raw)}
    validated_tls({"fixture_root": str(root), "receipt": str(workspace / "receipt.json"), "tls": tls}, end, clock, platform)
    return tls


def helper_plan(metadata, destination, hard, clock=time.monotonic):
    operation = hard.reserve(CLEANUP_RESERVE)
    operation.remaining()
    demand(sys.implementation.name == "cpython" and (3, 11) <= sys.version_info[:2] <= (3, 14),
           "Acquisition interpreter lifecycle unsupported")
    root, destination = fixture_binding(destination)
    metadata = reviewed_metadata(metadata, destination)
    interpreter = Path(sys.executable).resolve()
    worker = HERE / "acquisition_worker.py"
    demand(interpreter.is_absolute() and interpreter.is_file() and worker.is_file() and not worker.is_symlink(),
           "Trusted acquisition interpreter/helper unavailable")
    workspace = contained(root, destination.with_name(destination.name + ".acquisition"))
    demand(not workspace.exists(), "Acquisition workspace already exists; preserve incomplete evidence")
    workspace.mkdir(mode=0o700)
    home = workspace / "home"
    home.mkdir(mode=0o700)
    tls = acquisition_tls(root, workspace, operation.end, clock)
    request = {"version": 1, "metadata": metadata, "fixture_root": str(root), "archive": str(destination),
               "receipt": str(workspace / "receipt.json"), "operation_deadline": operation.end,
               "worker_sha256": file_digest(worker, operation.end, clock), "interpreter": str(interpreter),
               "interpreter_sha256": file_digest(interpreter, operation.end, clock), "tls": tls}
    request_path = workspace / "request.json"
    exclusive_json(request_path, request)
    request_path.chmod(0o400)
    raw, stored = bounded_json(request_path, MAX_REQUEST)
    demand(stored == request, "Acquisition request write differs")
    operation.remaining()
    return {"argv": [str(interpreter), "-I", "-S", "-B", str(worker), str(request_path),
                     hashlib.sha256(raw).hexdigest()], "cwd": str(workspace), "env": helper_environment(home),
            "request": request, "request_path": request_path, "destination": destination,
            "receipt_path": workspace / "receipt.json", "operation": operation}


def close_process_handles(process, platform=sys.platform):
    for name in ("stdin", "stdout", "stderr"):
        handle = getattr(process, name, None)
        if handle is not None:
            handle.close()
            demand(handle.closed, "Acquisition stream handle closure unconfirmed")
    if platform == "win32":
        handle = getattr(process, "_handle", None)
        demand(handle is not None and callable(getattr(handle, "Close", None)),
               "Windows acquisition process handle closure unsupported")
        handle.Close()
        demand(handle.closed, "Windows acquisition process handle closure unconfirmed")


class AcquisitionProcess:
    def __init__(self, plan, hard, factory=subprocess.Popen, event_factory=threading.Event,
                 thread_factory=threading.Thread, close_handles=close_process_handles):
        self.plan, self.hard, self.factory, self.close_handles = plan, hard, factory, close_handles
        self.started, self.finished, self.cancelled = (event_factory() for _ in range(3))
        self.lock = threading.Lock()
        self.process, self.exit_code, self.error = None, None, None
        self.exit_observed, self.handles_closed = False, False
        self.kill_error, self.kill_attempted = None, False
        self.thread = thread_factory(target=self.launch_and_wait, daemon=False)

    def cancel(self):
        self.cancelled.set()
        process = None
        with self.lock:
            if self.process is not None and not self.exit_observed and not self.kill_attempted:
                self.kill_attempted = True
                process = self.process
        if process is not None:
            try:
                process.kill()
            except BaseException as error:
                self.kill_error = error

    def launch_and_wait(self):
        try:
            if self.cancelled.is_set() or self.hard.clock() >= self.plan["operation"].end:
                raise RuntimeError("Acquisition cancelled before process creation")
            process = self.factory(self.plan["argv"], cwd=self.plan["cwd"], env=self.plan["env"], shell=False,
                                   close_fds=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL)
            with self.lock:
                self.process = process
            if self.cancelled.is_set() or self.hard.clock() >= self.plan["operation"].end:
                self.cancel()
            self.started.set()
            code = process.wait()
            with self.lock:
                self.exit_code, self.exit_observed = code, True
        except BaseException as error:
            self.error = error
            self.cancel()
        finally:
            self.started.set()
            self.finished.set()

    def start(self):
        self.plan["operation"].remaining()
        try:
            self.thread.start()
        except BaseException:
            self.cancel()
            raise
        demand(self.started.wait(self.plan["operation"].remaining()), "Acquisition startup unconfirmed")
        self.plan["operation"].remaining()
        demand(self.error is None, "Acquisition process start failed")
        demand(not self.cancelled.is_set(), "Acquisition was cancelled")

    def wait_exit(self):
        demand(self.finished.wait(self.plan["operation"].remaining()), "Acquisition process exit unconfirmed")
        self.plan["operation"].remaining()
        demand(self.exit_observed and self.process is not None and self.error is None,
               "Acquisition process exit unconfirmed")
        demand(not self.cancelled.is_set(), "Acquisition was cancelled")
        demand(self.exit_code == 0, "Acquisition helper failed")

    def finish(self, cancel):
        if cancel:
            self.cancel()
        try:
            if not self.finished.is_set():
                budget = self.hard.end - self.hard.clock()
                demand(budget > 0 and self.finished.wait(budget), "Acquisition owned process termination unconfirmed")
            demand(self.exit_observed and self.process is not None and self.error is None,
                   "Acquisition owned process termination unconfirmed")
            budget = self.hard.end - self.hard.clock()
            demand(budget > 0, "Acquisition cleanup deadline exceeded")
            self.thread.join(timeout=budget)
            demand(not self.thread.is_alive(), "Acquisition waiter termination unconfirmed")
            self.hard.remaining()
        finally:
            if self.exit_observed and self.process is not None:
                try:
                    self.close_handles(self.process)
                    self.handles_closed = True
                except BaseException as error:
                    self.error = error
                    raise RuntimeError("Acquisition handle closure unconfirmed") from error
        self.hard.remaining()

    def snapshot(self):
        return {"hard_deadline": self.hard.end, "recorded_at": self.hard.clock(),
                "operation_deadline": self.plan["operation"].end, "process_captured": self.process is not None, "exit_observed": self.exit_observed,
                "exit_code": self.exit_code, "handles_closed": self.handles_closed,
                "waiter_finished": self.finished.is_set(), "waiter_alive": self.thread.is_alive(),
                "cancel_requested": self.cancelled.is_set(), "kill_attempted": self.kill_attempted,
                "kill_error_type": type(self.kill_error).__name__ if self.kill_error else None,
                "error_type": type(self.error).__name__ if self.error else None}


def validated_result(plan, hard):
    hard.remaining()
    contained(plan["request"]["fixture_root"], plan["request_path"])
    contained(plan["request"]["fixture_root"], plan["receipt_path"])
    raw, request = bounded_json(plan["request_path"], MAX_REQUEST)
    demand(request == plan["request"] and hashlib.sha256(raw).hexdigest() == plan["argv"][-1],
           "Acquisition request changed")
    validated_tls(request, hard.end, hard.clock)
    _, receipt = bounded_json(plan["receipt_path"], MAX_RECEIPT)
    demand(isinstance(receipt, dict) and set(receipt) == {"version", "status", "request_sha256", "archive", "result"}
           and type(receipt["version"]) is int and receipt["version"] == 1 and receipt["status"] == "completed" and
           receipt["request_sha256"] == plan["argv"][-1] and receipt["archive"] == str(plan["destination"]),
           "Acquisition receipt envelope differs")
    result = receipt["result"]
    demand(isinstance(result, dict) and set(result) == {"sha512", "sha256", "bytes", "source", "final_url", "redirects"},
           "Acquisition result fields differ")
    demand(isinstance(result["redirects"], list) and len(result["redirects"]) <= 5,
           "Acquisition redirect receipt exceeds bound")
    demand(type(result["bytes"]) is int and 0 <= result["bytes"] <= 512 * 1024 * 1024,
           "Acquisition byte receipt invalid")
    demand(result["final_url"] == (result["redirects"][-1] if result["redirects"] else request["metadata"]["tarball"]),
           "Acquisition final URL/redirect join differs")
    for url in [result["final_url"], *result["redirects"]]:
        registry_url(url)
    from native_archive import verify_archive
    contained(request["fixture_root"], plan["destination"])
    demand(plan["destination"].is_file(), "Acquisition archive unavailable")
    actual = verify_archive(plan["destination"], request["metadata"])
    demand(all(result[key] == value for key, value in actual.items()), "Acquisition bytes differ from receipt")
    demand(file_digest(HERE / "acquisition_worker.py", hard.end, hard.clock) == request["worker_sha256"] and
           file_digest(request["interpreter"], hard.end, hard.clock) == request["interpreter_sha256"],
           "Trusted acquisition inputs changed")
    validated_tls(request, hard.end, hard.clock)
    hard.remaining()
    return actual


def acquire_owned(metadata, destination, deadline, clock=time.monotonic, plan_factory=helper_plan,
                  lifecycle_factory=AcquisitionProcess):
    demand(math.isfinite(deadline), "Acquisition requires finite original deadline")
    hard = Deadline(deadline, clock)
    plan = plan_factory(metadata, destination, hard, clock)
    lifecycle = lifecycle_factory(plan, hard)
    successful = False
    try:
        lifecycle.start()
        lifecycle.wait_exit()
        successful = True
    finally:
        try:
            lifecycle.finish(cancel=not successful)
        finally:
            exclusive_json(plan["receipt_path"].with_name("parent.json"),
                           {"version": 1, "request_sha256": plan["argv"][-1], "lifecycle": lifecycle.snapshot()})
    return validated_result(plan, hard)
