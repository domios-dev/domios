"""Fixed native --version request, bounded evidence and direct-resource qualification."""

import base64
import hashlib
import json
import math
import os
from pathlib import Path
import re
import stat
import time
import uuid

from deadline_io import Deadline
from launch_env import isolated_environment, make_home
from native_layout import demand
from native_version_platform import error_fact, owner_for, preflight

SCOPE = "direct_native_version_and_owned_handles"
CAP = 32768
HARD_SECONDS = 10
CLEANUP_SECONDS = 2


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")


def digest(value):
    return hashlib.sha256(value).hexdigest()


def input_digest(manifest):
    return digest(encoded({key: value for key, value in manifest.items() if key != "version_receipt"}))


def file_identity(path):
    value = Path(path).stat(follow_symlinks=False)
    demand(stat.S_ISREG(value.st_mode), "Native executable is not a regular file")
    return {"device": value.st_dev, "inode": value.st_ino, "size": value.st_size,
            "mode": value.st_mode, "mtime_ns": value.st_mtime_ns, "ctime_ns": value.st_ctime_ns}


def checked_file_hash(path, deadline):
    deadline.remaining()
    result = hashlib.sha256()
    with Path(path).open("rb", buffering=0) as stream:
        while True:
            deadline.remaining()
            chunk = stream.read(65536)
            deadline.remaining()
            if not chunk:
                break
            result.update(chunk)
    deadline.remaining()
    return result.hexdigest()


def contained(root, path):
    root, path = Path(root), Path(path)
    demand(root.is_absolute() and path.is_absolute() and root == root.resolve() and path == path.resolve(),
           "Native qualification paths must be absolute and resolved")
    demand(path != root and root in path.parents, "Native qualification path escapes fixture")
    return path


def environment_for(root, home, platform, provider, package_root=None, system_root=None):
    environment = isolated_environment({}, root, home)
    if platform == "win32":
        system_root = Path(system_root or "")
        demand(system_root.is_absolute() and system_root.is_dir() and system_root == system_root.resolve() and
               (system_root / "System32" / "kernel32.dll").is_file(), "Trusted Windows SystemRoot unavailable")
        environment["SystemRoot"] = str(system_root)
    if provider == "codex":
        demand(package_root is not None, "Verified managed package root absent")
        environment.update(CODEX_MANAGED_BY_NPM="1", CODEX_MANAGED_PACKAGE_ROOT=str(contained(root, package_root)))
    return environment


def request_for(root, directory, manifest, executable, identity, environment, started, operation_id=None):
    directory, executable = contained(root, directory), contained(directory, executable)
    return {"schema": 2, "operation_id": operation_id or str(uuid.uuid4()), "scope": SCOPE,
            "root": str(root), "instance": identity["instance"], "port": identity["port"],
            "provider": manifest["provider"], "version": manifest["version"], "platform": manifest["platform"],
            "arch": manifest["arch"], "directory": str(directory), "executable": str(executable),
            "argv": [str(executable), "--version"], "executable_sha256": manifest["executable_sha256"],
            "file_identity": file_identity(executable), "assembly_input_sha256": input_digest(manifest),
            "cwd": str(directory / "qualification"), "home": str(directory / "qualification-home"),
            "environment_sha256": digest(encoded(environment)), "environment_keys": sorted(environment),
            "started_at": started, "hard_deadline": started + HARD_SECONDS,
            "operation_deadline": started + HARD_SECONDS - CLEANUP_SECONDS,
            "cleanup_reserved_seconds": CLEANUP_SECONDS, "stdout_cap": CAP, "stderr_cap": CAP,
            "read_size": 4096}


def validate_request(request):
    demand(request.get("schema") == 2 and request.get("scope") == SCOPE, "Native request schema differs")
    root = Path(request["root"])
    directory = contained(root, request["directory"])
    executable = contained(directory, request["executable"])
    demand(request["argv"] == [str(executable), "--version"] and
           request["cwd"] == str(directory / "qualification") and
           request["home"] == str(directory / "qualification-home"), "Native argv/cwd/home differs")
    demand(str(uuid.UUID(request["operation_id"])) == request["operation_id"], "Native operation identity malformed")
    demand(request["provider"] in ("claude", "codex", "opencode") and
           re.fullmatch(r"\d+\.\d+\.\d+", request["version"]) and
           request["platform"] in ("darwin", "linux", "win32") and request["arch"] in ("arm64", "x64") and
           re.fullmatch(r"[A-Za-z0-9_-]+", request["instance"]) and
           type(request["port"]) is int and 1024 <= request["port"] <= 65535,
           "Native fixture identity malformed")
    demand(directory == root / "native" / request["provider"] / request["version"], "Native directory identity differs")
    for key in ("executable_sha256", "assembly_input_sha256", "environment_sha256"):
        demand(re.fullmatch(r"[a-f0-9]{64}", request[key]) is not None, "Native request hash malformed")
    demand(set(request["file_identity"]) == {"device", "inode", "size", "mode", "mtime_ns", "ctime_ns"} and
           all(type(value) is int for value in request["file_identity"].values()), "Native file identity malformed")
    expected_keys = set(isolated_environment({}, root, Path(request["home"])))
    if request["platform"] == "win32":
        expected_keys.add("SystemRoot")
    if request["provider"] == "codex":
        expected_keys.update(("CODEX_MANAGED_BY_NPM", "CODEX_MANAGED_PACKAGE_ROOT"))
    demand(request["environment_keys"] == sorted(expected_keys), "Native environment keys differ")
    for key in ("started_at", "operation_deadline", "hard_deadline"):
        demand(type(request[key]) in (int, float) and math.isfinite(request[key]), "Native deadline malformed")
    demand(request["hard_deadline"] == request["started_at"] + HARD_SECONDS and
           request["operation_deadline"] == request["hard_deadline"] - CLEANUP_SECONDS and
           request["cleanup_reserved_seconds"] == CLEANUP_SECONDS and
           request["stdout_cap"] == request["stderr_cap"] == CAP and request["read_size"] == 4096,
           "Native original deadline or bounds changed")


class Capture:
    def __init__(self):
        self.data = bytearray()
        self.eof = self.overflow = self.cancelled = False
        self.error = None
        self.eof_at = None

    def capacity(self):
        return min(4096, CAP + 1 - len(self.data))

    def accept(self, value, observed_at):
        demand(not self.eof and self.error is None and not self.cancelled, "Native capture event after retirement")
        if isinstance(value, dict):
            if value.get("type") == "cancelled":
                self.cancelled = True
            else:
                self.error = value
        else:
            demand(type(value) is bytes and len(value) <= self.capacity(), "Native capture chunk exceeds bound")
            if not value:
                self.eof = True
                self.eof_at = observed_at
            elif len(self.data) + len(value) > CAP:
                self.data.extend(value[:CAP - len(self.data)])
                self.overflow = True
            else:
                self.data.extend(value)

    def facts(self, request_hash):
        return {"request_sha256": request_hash, "bytes": len(self.data), "sha256": digest(self.data),
                "base64": base64.b64encode(self.data).decode("ascii"), "eof": self.eof,
                "overflow": self.overflow, "cancelled": self.cancelled, "error": self.error, "eof_at": self.eof_at}


def execute_request(request, environment, clock=time.monotonic, owner_factory=owner_for, event_sink=None, resource_sink=None):
    validate_request(request)
    hard = Deadline(request["hard_deadline"], clock)
    operation = Deadline(request["operation_deadline"], clock)
    request_hash = digest(encoded(request))
    captures = {name: Capture() for name in ("stdout", "stderr")}
    owner, failure = None, None
    try:
        operation.remaining()
        demand(digest(encoded(environment)) == request["environment_sha256"], "Native environment request changed")
        owner = owner_factory(request, environment, clock)
        operation.remaining()
        owner.start()
        while True:
            operation.remaining()
            facts = owner.facts()
            if resource_sink is not None:
                resource_sink(facts, request_hash, clock())
            demand(facts["error"] is None and not facts["ownership_lost"] and not facts["cancel_requested"],
                   "Native owned process failed or cancelled")
            if facts["exit_observed"] and all(value.eof for value in captures.values()):
                break
            events = owner.next(operation, {name: value.capacity() for name, value in captures.items()})
            operation.remaining()
            for name, value in events:
                if name == "wake":
                    continue
                demand(name in captures, "Native capture stream unknown")
                captures[name].accept(value, clock())
                if event_sink is not None:
                    event_sink(name, captures[name].facts(request_hash), clock())
                capture = captures[name]
                demand(not capture.overflow and not capture.cancelled and capture.error is None,
                       "Native bounded capture failed")
    except BaseException as error:
        failure = error_fact(error)
    if owner is not None:
        try:
            owner.finish(hard, cancel=failure is not None)
        except BaseException as error:
            failure = failure or error_fact(error)
            owner.cancel()
        resources = owner.facts()
    else:
        resources = {"captured": False, "exit_observed": False, "exit_collected": False, "exit_code": None,
                     "ownership_lost": False, "cancel_requested": False, "signal_attempted": False,
                     "error": failure, "kind": None, "reaped": None, "waiter_finished": False,
                     "waiter_joined": False, "capture_completed": False, "handles_closed": False}
    resources["request_sha256"] = request_hash
    if resource_sink is not None:
        try:
            resource_sink(resources, request_hash, clock())
        except BaseException as error:
            failure = failure or error_fact(error)
    result = {"schema": 2, "scope": SCOPE, "descendants": "not_proven", "request": request,
              "request_sha256": request_hash, "resources": resources,
              "streams": {name: value.facts(request_hash) for name, value in captures.items()},
              "error": failure, "completed_at": clock(), "status": "failed",
              "version": None, "executable_sha256": request["executable_sha256"],
              "argv": request["argv"], "exit_code": resources["exit_code"], "inputs_revalidated": False}
    try:
        text = bytes(captures["stdout"].data).decode("utf-8", errors="strict")
        versions = re.findall(r"(?<![\w.])\d+\.\d+\.\d+(?![\w.])", text)
        result["version"] = versions[0] if len(versions) == 1 else None
    except UnicodeError:
        result["error"] = result["error"] or {"type": "UnicodeError", "errno": None}
    return result


class CaptureJournal:
    def __init__(self, path, deadline):
        deadline.remaining()
        self.stream = Path(path).open("xb", buffering=0)
        self.deadline, self.count = deadline, 0
        self.offsets = {"stdout": 0, "stderr": 0}
        self.previous_resources = None

    def resource(self, facts, request_hash, observed_at):
        record = encoded(facts)
        if record != self.previous_resources:
            self.frame({"resources": facts, "observed_at": observed_at, "request_sha256": request_hash})
            self.previous_resources = record

    def frame(self, row):
        self.deadline.remaining()
        frame = encoded(row) + b"\n"
        demand(len(frame) <= 8192 and self.count + len(frame) <= 262144, "Native event journal cap exceeded")
        offset = 0
        while offset < len(frame):
            self.deadline.remaining()
            written = self.stream.write(frame[offset:offset + 4096])
            demand(type(written) is int and written > 0, "Native event journal incomplete")
            offset += written
        self.count += len(frame)
        self.deadline.remaining()

    def append(self, name, facts, observed_at):
        self.deadline.remaining()
        raw = base64.b64decode(facts["base64"], validate=True)
        start = self.offsets[name]
        row = {"stream": name, "offset": start, "base64": base64.b64encode(raw[start:]).decode("ascii"),
               "observed_at": observed_at, "request_sha256": facts["request_sha256"],
               "eof": facts["eof"], "overflow": facts["overflow"], "error": facts["error"],
               "cancelled": facts["cancelled"]}
        self.frame(row)
        self.offsets[name] = len(raw)
        self.deadline.remaining()

    def close(self):
        self.stream.close()
        demand(self.stream.closed, "Native event journal close unconfirmed")


def validate_receipt(receipt, version, executable, executable_hash):
    demand(type(receipt) is dict and receipt.get("schema") == 2 and receipt.get("status") == "verified" and
           receipt.get("scope") == SCOPE and receipt.get("descendants") == "not_proven" and
           receipt.get("error") is None and receipt.get("inputs_revalidated") is True,
           "Native executable version remains unqualified")
    request = receipt["request"]
    validate_request(request)
    request_hash = digest(encoded(request))
    demand(receipt["request_sha256"] == request_hash and receipt["version"] == request["version"] == version and
           receipt["argv"] == request["argv"] == [str(executable), "--version"] and
           receipt["executable_sha256"] == request["executable_sha256"] == executable_hash and
           receipt["exit_code"] == 0 and type(receipt["exit_code"]) is int,
           "Native executable version remains unqualified")
    resources = receipt["resources"]
    demand(resources["request_sha256"] == request_hash and resources["error"] is None and
           resources["ownership_lost"] is False and resources["cancel_requested"] is False and
           resources["signal_attempted"] is False and type(resources["exit_code"]) is int and resources["exit_code"] == 0,
           "Native resource receipt remains unqualified")
    for key in ("captured", "exit_observed", "exit_collected", "waiter_finished", "waiter_joined",
                "capture_completed", "handles_closed"):
        demand(resources[key] is True, "Native owned resource completion remains unqualified")
    demand((request["platform"] == "win32" and resources["kind"] == "win32_captured_process_handle" and
            resources["reaped"] is None) or (request["platform"] in ("darwin", "linux") and
            resources["kind"] == "unix_waitid_sole_reaper" and resources["reaped"] is True),
           "Native exit ownership remains unqualified")
    demand(resources["cancel_requested_at"] is None and resources["signal_succeeded"] is None and
           resources["signal_finished_at"] is None, "Native cancellation receipt remains unqualified")
    for key in ("captured_at", "exit_observed_at", "exit_collected_at", "waiter_joined_at", "handles_closed_at"):
        demand(type(resources[key]) in (int, float) and math.isfinite(resources[key]) and
               request["started_at"] <= resources[key] <= receipt["completed_at"],
               "Native resource time remains unqualified")
    demand(resources["captured_at"] <= resources["exit_observed_at"] <= resources["exit_collected_at"] <=
           resources["waiter_joined_at"] <= resources["handles_closed_at"] and
           resources["captured_at"] < request["operation_deadline"] and
           resources["exit_observed_at"] < request["operation_deadline"], "Native lifecycle ordering remains unqualified")
    demand(type(receipt["completed_at"]) in (int, float) and math.isfinite(receipt["completed_at"]) and
           request["started_at"] <= receipt["completed_at"] < request["hard_deadline"],
           "Native completion missed original deadline")
    demand(set(receipt["streams"]) == {"stdout", "stderr"}, "Native capture streams differ")
    for stream in receipt["streams"].values():
        demand(type(stream["base64"]) is str and len(stream["base64"]) <= 4 * ((CAP + 2) // 3),
               "Native capture encoding remains unqualified")
        raw = base64.b64decode(stream["base64"], validate=True)
        demand(stream["request_sha256"] == request_hash and type(stream["bytes"]) is int and
               stream["bytes"] == len(raw) <= CAP and stream["sha256"] == digest(raw) and
               stream["eof"] is True and stream["error"] is None and stream["overflow"] is False and
               stream["cancelled"] is False, "Native capture remains unqualified")
        demand(type(stream["eof_at"]) in (int, float) and math.isfinite(stream["eof_at"]) and
               request["started_at"] <= stream["eof_at"] < request["operation_deadline"],
               "Native EOF deadline remains unqualified")
    text = base64.b64decode(receipt["streams"]["stdout"]["base64"], validate=True).decode("utf-8", errors="strict")
    demand(re.findall(r"(?<![\w.])\d+\.\d+\.\d+(?![\w.])", text) == [version],
           "Native parsed version remains unqualified")


def write_exclusive(path, value, deadline):
    deadline.remaining()
    data = value if type(value) is bytes else encoded(value)
    demand(len(data) <= 262144, "Native sidecar byte limit exceeded")
    with Path(path).open("xb", buffering=0) as stream:
        offset = 0
        while offset < len(data):
            deadline.remaining()
            written = stream.write(data[offset:offset + 4096])
            demand(type(written) is int and written > 0, "Native sidecar write incomplete")
            offset += written
        deadline.remaining()
    deadline.remaining()
    return digest(data)


def read_sidecar(path):
    with Path(path).open("rb") as stream:
        data = stream.read(262145)
    demand(len(data) <= 262144, "Native sidecar byte limit exceeded")
    return json.loads(data), digest(data)


def validate_sidecar(directory, reference, manifest):
    demand(reference.get("status") == "verified" and reference.get("sidecar") == "qualification/receipt.json" and
           reference.get("completion") == "qualification/completion.json", "Native receipt sidecar remains unqualified")
    receipt, receipt_hash = read_sidecar(contained(directory, directory / reference["sidecar"]))
    completion, completion_hash = read_sidecar(contained(directory, directory / reference["completion"]))
    request, request_hash = read_sidecar(contained(directory, directory / "qualification/request.json"))
    before, _ = read_sidecar(contained(directory, directory / "qualification/assembly-before.json"))
    demand(request == receipt["request"] and request_hash == receipt["request_sha256"] and
           input_digest(before) == input_digest(manifest), "Native retained inputs remain unqualified")
    demand(reference["sha256"] == receipt_hash and reference["completion_sha256"] == completion_hash and
           completion["receipt_sha256"] == receipt_hash and completion["request_sha256"] == receipt["request_sha256"] and
           completion["assembly_input_sha256"] == input_digest(manifest) == receipt["request"]["assembly_input_sha256"] and
           receipt["completed_at"] <= completion["recorded_at"] < receipt["request"]["hard_deadline"] and
           reference["finalized_at"] >= completion["recorded_at"] and
           reference["finalized_at"] < receipt["request"]["hard_deadline"], "Native sidecar joins remain unqualified")
    return receipt


def qualify_native(root, provider, version, load, manifest_path, host_arch, clock=time.monotonic):
    started = clock()
    hard, operation = Deadline(started + HARD_SECONDS, clock), Deadline(started + HARD_SECONDS - CLEANUP_SECONDS, clock)
    operation.remaining()
    manifest, executable = load(root, provider, version, qualified=False, deadline=operation)
    operation.remaining()
    demand(manifest["platform"] == os.sys.platform and manifest["arch"] == host_arch(), "Wrong native host")
    preflight(manifest["platform"])
    operation.remaining()
    path = manifest_path(root, provider, version)
    directory = path.parent
    sidecar, home = directory / "qualification", directory / "qualification-home"
    demand(not sidecar.exists() and not home.exists(), "Native qualification attempt already exists; preserve evidence")
    sidecar.mkdir(mode=0o700)
    home.mkdir(mode=0o700)
    make_home(root, home)
    operation.remaining()
    before_bytes = path.read_bytes()
    demand(json.loads(before_bytes) == manifest, "Native manifest changed before qualification")
    write_exclusive(sidecar / "assembly-before.json", before_bytes, operation)
    identity = json.loads((root / "instance.json").read_text())
    package_root = directory / "wrapper" if provider == "codex" else None
    if package_root is not None:
        package = json.loads((package_root / "package.json").read_text())
        demand(package["name"] == "@openai/codex" and package["version"] == version, "Managed package identity changed")
    environment = environment_for(root, home, manifest["platform"], provider, package_root,
                                  os.environ.get("SystemRoot") if manifest["platform"] == "win32" else None)
    request = request_for(root, directory, manifest, executable, identity, environment, started)
    operation.remaining()
    write_exclusive(sidecar / "request.json", request, operation)
    write_exclusive(sidecar / "incomplete.json", {"status": "incomplete", "request_sha256": digest(encoded(request))}, operation)
    demand(file_identity(executable) == request["file_identity"] and
           checked_file_hash(executable, operation) == request["executable_sha256"] and
           path.read_bytes() == before_bytes, "Native input changed before launch")
    journal = CaptureJournal(sidecar / "capture.jsonl", hard)
    try:
        result = execute_request(request, environment, clock, event_sink=journal.append, resource_sink=journal.resource)
    finally:
        journal.close()
    try:
        hard.remaining()
        after, after_executable = load(root, provider, version, qualified=False, deadline=hard)
        demand(after == manifest and after_executable == executable and path.read_bytes() == before_bytes and
               file_identity(executable) == request["file_identity"] and
               checked_file_hash(executable, hard) == request["executable_sha256"], "Native input changed after execution")
        result["inputs_revalidated"] = True
        result["status"] = "verified"
        validate_receipt(result, version, executable, manifest["executable_sha256"])
    except BaseException as error:
        result["status"] = "failed"
        result["error"] = result["error"] or error_fact(error)
    receipt_hash = write_exclusive(sidecar / "receipt.json", result, hard)
    completion = {"receipt_sha256": receipt_hash, "request_sha256": result["request_sha256"],
                  "assembly_input_sha256": input_digest(manifest), "recorded_at": clock()}
    completion_hash = write_exclusive(sidecar / "completion.json", completion, hard)
    reference = {"status": result["status"], "sidecar": "qualification/receipt.json", "sha256": receipt_hash,
                 "completion": "qualification/completion.json", "completion_sha256": completion_hash,
                 "finalized_at": clock()}
    hard.remaining()
    demand(path.read_bytes() == before_bytes, "Native manifest changed before receipt update")
    manifest["version_receipt"] = reference
    from fixtures import write_json
    write_json(path, manifest)
    try:
        hard.remaining()
    except BaseException:
        # Publication that returns late must leave a refusal, never a verified manifest.
        manifest["version_receipt"] = reference | {"status": "unconfirmed"}
        write_json(path, manifest)
        raise
    demand(result["status"] == "verified", "Native executable version remains unqualified")
    return result
