"""One sequential Jev helper lease with exact owned exit and channel receipts."""

from dataclasses import dataclass
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import signal
import stat
import struct
import subprocess
import sys
import tempfile
import threading
import time

import request_contract as contract
import jev_request_worker as worker_support

MAX_CALLS = 300
REQUEST_SECONDS = 60
RUN_SECONDS = 600
CLEANUP_SECONDS = 5
MAX_CAPTURE = 4096
CHUNK = 4096


class TransportFailure(ValueError):
    def __init__(self, code, receipt=None, *, owner=None):
        self.code, self.receipt, self.owner = code, receipt or {}, owner
        super().__init__(code)


@dataclass(frozen=True)
class TransportResult:
    payload: dict
    receipt: dict


def require(value, code):
    if not value:
        raise TransportFailure(code)


def remaining(end, clock):
    value = end - clock()
    require(math.isfinite(end) and value > 0, "deadline")
    return value


def request_deadlines(start, run_end, timeout, *, request_end=None):
    require(type(timeout) in (int, float) and math.isfinite(timeout) and CLEANUP_SECONDS < timeout <= REQUEST_SECONDS,
            "request_budget")
    try:
        valid_end = request_end is None or type(request_end) in (int, float) and math.isfinite(request_end)
    except OverflowError:
        valid_end = False
    require(valid_end, "request_end_bound")
    hard_end = min(start + timeout, run_end) if request_end is None else min(start + timeout, run_end, request_end)
    require(hard_end - start > CLEANUP_SECONDS, "run_deadline")
    return hard_end - CLEANUP_SECONDS, hard_end


def token_bytes(token):
    require(type(token) is str and 0 < len(token) <= CHUNK and all(33 <= ord(char) <= 126 for char in token),
            "token_preflight")
    return token.encode("ascii")


def minimal_environment(*, platform=None):
    platform = os.name if platform is None else platform
    if platform == "nt":
        buffer = ctypes.create_unicode_buffer(32768)
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.GetWindowsDirectoryW.argtypes = (ctypes.c_void_p, ctypes.c_uint32)
        kernel.GetWindowsDirectoryW.restype = ctypes.c_uint32
        length = kernel.GetWindowsDirectoryW(buffer, len(buffer))
        require(0 < length < len(buffer), "windows_system_root")
        return {"SystemRoot": buffer.value}
    return {"LANG": "C"}


def identity_hash(path, end, clock):
    if os.name == "nt":
        guard, result, failure = None, None, None
        try:
            guard = worker_support.WindowsTrust(end, clock=clock)
            exact, digest, _ = guard.file(path)
            result = exact, digest
        except BaseException as error:
            failure = error
            guard = guard or getattr(error, "owner", None)
        facts = {**getattr(failure, "facts", {}), "trust_handles_closed": guard is not None and guard.closed}
        if guard is not None:
            try:
                guard.close()
                facts["trust_handles_closed"] = guard.closed
            except BaseException as error:
                facts.update(trust_handles_closed=guard.closed, trust_cleanup_error=getattr(error, "code", "windows_trust_close"))
                failure = failure or error
        if failure is not None:
            if guard is not None:
                facts["windows_trust"] = guard.receipt()
            raise TransportFailure(getattr(failure, "code", "windows_file_hash"), facts, owner=guard) from failure
        return result
    path = Path(path).resolve(strict=True)
    before = path.stat()
    require(stat.S_ISREG(before.st_mode) and not before.st_mode & 0o022 and
            before.st_uid in (0, os.getuid()), "trusted_file_permissions")
    fields = ("st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_size", "st_mtime_ns", "st_ctime_ns")
    signature = tuple(getattr(before, name) for name in fields)
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        require(tuple(getattr(os.fstat(stream.fileno()), name) for name in fields) == signature,
                "trusted_file_changed")
        while True:
            remaining(end, clock)
            raw = stream.read(CHUNK)
            if not raw:
                break
            digest.update(raw)
    after = path.stat()
    require(signature == tuple(getattr(after, name) for name in fields), "trusted_file_changed")
    remaining(end, clock)
    return path, digest.hexdigest()


def native_support():
    require(sys.implementation.name == "cpython" and (3, 9) <= sys.version_info[:2] <= (3, 14),
            "unsupported_interpreter")
    if os.name == "nt":
        require(sys.platform == "win32" and sys.version_info[:3] == (3, 14, 6) and
                worker_support.sysconfig.get_platform() == "win-amd64", "unsupported_windows_runtime")
        return "nt"
    require(os.name == "posix" and sys.platform in ("linux", "darwin"), "private_directory_unsupported")
    if sys.platform == "linux":
        require(all(hasattr(os, name) for name in ("waitid", "P_PID", "WEXITED", "WNOWAIT")), "waitid_unsupported")
    return "posix"


def prepare_plan(wire, operation_end, hard_end, clock):
    platform = native_support()
    if platform == "nt":
        try:
            guard = worker_support.WindowsTrust(operation_end, clock=clock)
        except worker_support.WorkerError as error:
            raise TransportFailure(error.code, error.facts, owner=getattr(error, "owner", None)) from error
        try:
            worker, worker_hash, _ = guard.file(Path(__file__).with_name("jev_request_worker.py"))
            _, contract_hash, _ = guard.file(Path(__file__).with_name("request_contract.py"))
            interpreter, interpreter_hash, _ = guard.file(sys.executable)
            guard.runtime(interpreter)
            root = guard.create_private()
            environment = minimal_environment(platform="nt")
            guard.revalidate()
            header = {"version": 1, "operation_end": operation_end, "hard_end": hard_end,
                      "endpoint": contract.ENDPOINT, "wire_sha256": contract.sha256(wire),
                      "worker_sha256": worker_hash, "contract_sha256": contract_hash,
                      "interpreter": str(interpreter), "interpreter_sha256": interpreter_hash, "cwd": str(root)}
            return {"argv": [str(interpreter), "-I", "-S", "-B", str(worker)], "env": environment,
                    "cwd": root, "header": header, "windows_guard": guard}
        except BaseException as error:
            facts = {**getattr(error, "facts", {}), "trust_handles_closed": False,
                     "cwd_closed": guard.created_directory is None}
            try:
                guard.close(remove_cwd=guard.created_directory is not None, cleanup_end=hard_end)
                facts.update(trust_handles_closed=guard.closed, cwd_closed=guard.created_directory is None)
            except BaseException as cleanup:
                facts["preparation_cleanup_error"] = getattr(cleanup, "code", "preparation_cleanup_os")
            facts.update(trust_handles_closed=guard.closed, cwd_closed=guard.created_directory is None)
            trust_receipt = guard.receipt()
            if type(trust_receipt) is dict:
                facts["windows_trust"] = trust_receipt
            code = getattr(error, "code", "preparation_os")
            raise TransportFailure(code, facts, owner=guard) from error
    worker, worker_hash = identity_hash(Path(__file__).with_name("jev_request_worker.py"), operation_end, clock)
    _, contract_hash = identity_hash(Path(__file__).with_name("request_contract.py"), operation_end, clock)
    interpreter, interpreter_hash = identity_hash(sys.executable, operation_end, clock)
    root = Path(tempfile.mkdtemp(prefix="jev-owned-")).resolve()
    try:
        mode = root.stat()
        require(not root.is_symlink() and mode.st_uid == os.getuid() and stat.S_IMODE(mode.st_mode) == 0o700,
                "private_directory")
        remaining(operation_end, clock)
        header = {"version": 1, "operation_end": operation_end, "hard_end": hard_end,
                  "endpoint": contract.ENDPOINT, "wire_sha256": contract.sha256(wire),
                  "worker_sha256": worker_hash, "contract_sha256": contract_hash,
                  "interpreter": str(interpreter), "interpreter_sha256": interpreter_hash, "cwd": str(root)}
        return {"argv": [str(interpreter), "-I", "-S", "-B", str(worker)],
                "env": minimal_environment(), "cwd": root, "header": header}
    except BaseException:
        shutil.rmtree(root)
        raise


def spawn_helper(plan):
    return subprocess.Popen(plan["argv"], cwd=plan["cwd"], env=plan["env"],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            close_fds=True, bufsize=0)


def observe_unreaped_exit(pid):
    if sys.platform == "darwin":
        # Darwin SDK wait.h pins P_PID=1, WEXITED=4, WNOWAIT=0x20; siginfo is never decoded.
        libc = ctypes.CDLL(None, use_errno=True)
        waitid = libc.waitid
        waitid.argtypes = (ctypes.c_int, ctypes.c_uint, ctypes.c_void_p, ctypes.c_int)
        waitid.restype = ctypes.c_int
        storage = ctypes.create_string_buffer(1024)
        result = waitid(1, pid, ctypes.byref(storage), 0x4 | 0x20)
        if result != 0:
            raise OSError(ctypes.get_errno(), "owned waitid failed")
    else:
        observed = os.waitid(os.P_PID, pid, os.WEXITED | os.WNOWAIT)
        require(observed is not None and observed.si_pid == pid, "exit_identity")


class ProcessAuthority:
    def __init__(self, process, *, observer=observe_unreaped_exit, reaper=None, signaler=os.kill,
                 windows=None, status_converter=None, kill_signal=None):
        self.process, self.observer, self.signaler = process, observer, signaler
        self.reaper = getattr(os, "waitpid", None) if reaper is None else reaper
        self.status_converter = getattr(os, "waitstatus_to_exitcode", None) if status_converter is None else status_converter
        self.kill_signal = getattr(signal, "SIGKILL", None) if kill_signal is None else kill_signal
        self.lock = threading.Lock()
        self.reaped, self.lost, self.closed = False, False, False
        self.returncode = None
        self.signal_requested = False
        self.windows = os.name == "nt" if windows is None else windows
        self.handle = process._handle if self.windows else None
        if self.windows:
            require(callable(getattr(self.handle, "Close", None)) and not getattr(self.handle, "closed", False) and
                    0 < int(self.handle) != ctypes.c_void_p(-1).value, "captured_handle_unsupported")

    def wait_exact(self):
        try:
            if self.windows:
                kernel = ctypes.WinDLL("kernel32", use_last_error=True)
                kernel.WaitForSingleObject.argtypes = (ctypes.c_void_p, ctypes.c_uint32)
                kernel.WaitForSingleObject.restype = ctypes.c_uint32
                result = kernel.WaitForSingleObject(int(self.handle), 0xFFFFFFFF)
                require(result == 0, "exit_wait")
            else:
                self.observer(self.process.pid)
            with self.lock:
                require(not self.lost and not self.reaped, "reap_authority")
                if self.windows:
                    code = ctypes.c_uint32()
                    kernel.GetExitCodeProcess.argtypes = (ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32))
                    kernel.GetExitCodeProcess.restype = ctypes.c_int32
                    require(kernel.GetExitCodeProcess(int(self.handle), ctypes.byref(code)), "exit_code")
                    self.returncode = code.value
                else:
                    pid, status = self.reaper(self.process.pid, 0)
                    require(pid == self.process.pid, "reap_identity")
                    require(callable(self.status_converter), "posix_status_unsupported")
                    self.returncode = self.status_converter(status)
                self.process.returncode = self.returncode
                self.reaped = True
                return self.returncode
        except BaseException:
            with self.lock:
                self.lost = True
            raise

    def cancel(self):
        with self.lock:
            if self.reaped or self.lost or self.closed or self.signal_requested:
                return False
            self.signal_requested = True
            if self.windows:
                kernel = ctypes.WinDLL("kernel32", use_last_error=True)
                kernel.TerminateProcess.argtypes = (ctypes.c_void_p, ctypes.c_uint32)
                kernel.TerminateProcess.restype = ctypes.c_int32
                require(kernel.TerminateProcess(int(self.handle), 1), "cancel_os")
            else:
                require(self.kill_signal is not None, "posix_signal_unsupported")
                self.signaler(self.process.pid, self.kill_signal)
            return True

    def close(self):
        with self.lock:
            if self.closed:
                return
            require(self.reaped and not self.lost, "handle_unconfirmed")
            try:
                if self.windows:
                    self.handle.Close()
                self.closed = True
            except BaseException:
                self.lost = True
                raise


def settlement(facts):
    keys = ("start_finished", "created", "exit_observed", "reap_confirmed", "stdout_complete", "stderr_complete",
            "stdout_closed", "stderr_closed", "credential_channel_closed", "threads_joined", "handle_closed", "cwd_closed")
    return all(facts.get(key) is True for key in keys) and not facts.get("ownership_lost") and (
        not facts.get("windows_trust_required") or facts.get("trust_handles_closed") is True)


class OwnedLease:
    def __init__(self, plan, operation_end, hard_end, *, clock=time.monotonic, spawn=spawn_helper,
                 authority_factory=ProcessAuthority, thread_factory=threading.Thread, capture_source=None):
        self.plan, self.operation_end, self.hard_end = plan, operation_end, hard_end
        self.clock, self.spawn, self.authority_factory = clock, spawn, authority_factory
        self.thread_factory, self.capture_source = thread_factory, capture_source
        self.condition = threading.Condition()
        self.process, self.authority = None, None
        self.threads = []
        self.cancelled, self.input_started = False, False
        self.stdout = b""
        self.facts = {"operation_end": operation_end, "hard_end": hard_end,
                      "start_finished": False, "created": False, "exit_observed": False,
                      "reap_confirmed": False, "stdout_complete": False, "stderr_complete": False,
                      "stdout_closed": False, "stderr_closed": False, "credential_channel_closed": False,
                      "threads_joined": False, "handle_closed": False, "cwd_closed": False,
                      "ownership_lost": False, "cancel_requested": False, "returncode": None,
                      "stdout_bytes": 0, "stderr_bytes": 0, "error": None}
        if "windows_guard" in plan:
            self.facts.update(windows_trust_required=True, trust_handles_closed=False)

    def mark(self, **values):
        with self.condition:
            self.facts.update(values)
            self.condition.notify_all()

    def fail(self, code):
        with self.condition:
            if self.facts["error"] is None:
                self.facts["error"] = code
            self.condition.notify_all()

    def launch_thread(self, target, name, args=()):
        thread = self.thread_factory(target=target, name=name, args=args, daemon=False)
        self.threads.append(thread)
        thread.start()

    def start(self):
        self.launch_thread(self._create, "jev-create")

    def _create(self):
        try:
            remaining(self.operation_end, self.clock)
            with self.condition:
                require(not self.cancelled, "cancelled")
            process = self.spawn(self.plan)
            with self.condition:
                self.process = process
                self.facts["created"] = True
                self.authority = self.authority_factory(process)
            self.launch_thread(self._exit, "jev-exit")
            self.launch_thread(self._capture, "jev-stdout", ("stdout", process.stdout))
            self.launch_thread(self._capture, "jev-stderr", ("stderr", process.stderr))
            with self.condition:
                cancel = self.cancelled or self.clock() >= self.operation_end
            if cancel:
                self.cancel("deadline" if not self.cancelled else "cancelled")
                self._close_input()
        except BaseException as error:
            self.fail(error.code if isinstance(error, TransportFailure) else "creation_os")
            if self.authority is not None:
                self.cancel("creation_os")
                self._close_input()
        finally:
            self.mark(start_finished=True)

    def _exit(self):
        try:
            code = self.authority.wait_exact()
            self.mark(exit_observed=True, reap_confirmed=True, returncode=code)
        except BaseException:
            self.mark(ownership_lost=True)
            self.fail("exit_unconfirmed")

    def _capture(self, name, stream):
        captured, count = [], 0
        try:
            source = self.capture_source(name, stream) if self.capture_source else stream
            while True:
                raw = source.read(CHUNK)
                require(type(raw) is bytes and len(raw) <= CHUNK, "capture_read_shape")
                count += len(raw)
                self.mark(**{name + "_bytes": count})
                require(count <= MAX_CAPTURE, "capture_oversize")
                if not raw:
                    if name == "stdout":
                        self.stdout = b"".join(captured)
                    self.mark(**{name + "_complete": True})
                    break
                if name == "stdout":
                    captured.append(raw)
        except BaseException as error:
            self.fail(error.code if isinstance(error, TransportFailure) else "capture_os")
            self.cancel("capture_os")
        finally:
            try:
                stream.close()
                self.mark(**{name + "_closed": True})
            except BaseException:
                self.fail("capture_close")

    def _close_input(self):
        try:
            self.process.stdin.close()
            self.mark(credential_channel_closed=True)
        except BaseException:
            self.fail("channel_close")

    def _write_input(self, packet):
        try:
            offset = 0
            while offset < len(packet):
                remaining(self.operation_end, self.clock)
                with self.condition:
                    require(not self.cancelled, "cancelled")
                count = self.process.stdin.write(packet[offset:offset + CHUNK])
                require(type(count) is int and 0 < count <= CHUNK, "channel_write")
                offset += count
            remaining(self.operation_end, self.clock)
        except BaseException as error:
            self.fail(error.code if isinstance(error, TransportFailure) else "channel_os")
            self.cancel("channel_os")
        finally:
            self._close_input()

    def admit(self, wire, token):
        with self.condition:
            require(self.facts["start_finished"] and self.authority is not None and not self.cancelled and
                    not self.facts["error"] and not self.input_started and not self.facts["exit_observed"],
                    "credential_admission")
            remaining(self.operation_end, self.clock)
            header = contract.encode(self.plan["header"], MAX_CAPTURE)
            packet = b"".join(struct.pack("!I", len(part)) + part for part in (header, wire, token))
            self.input_started = True
            self.launch_thread(self._write_input, "jev-auth-channel", (packet,))

    def cancel(self, reason="cancelled"):
        with self.condition:
            self.cancelled = True
            self.facts["cancel_requested"] = True
            if self.facts["error"] is None:
                self.facts["error"] = reason
            authority = self.authority
            self.condition.notify_all()
        if authority is not None:
            try:
                authority.cancel()
            except BaseException:
                self.mark(cancel_error="cancel_os")
                self.fail("cancel_os")

    def await_state(self, predicate, end):
        with self.condition:
            while not predicate(self.facts):
                self.condition.wait(timeout=remaining(end, self.clock))
            remaining(end, self.clock)

    def streams_and_exit_done(self, facts):
        return facts["start_finished"] and facts["exit_observed"] and facts["stdout_closed"] and \
            facts["stderr_closed"] and facts["credential_channel_closed"]

    def finish(self):
        try:
            if self.cancelled and self.process is not None and not self.input_started:
                self._close_input()
            self.await_state(self.streams_and_exit_done, self.hard_end)
            for thread in self.threads:
                thread.join(timeout=remaining(self.hard_end, self.clock))
                require(not thread.is_alive(), "thread_unconfirmed")
            self.mark(threads_joined=True)
            self.authority.close()
            self.mark(handle_closed=True)
            if "windows_guard" in self.plan:
                guard = self.plan["windows_guard"]
                try:
                    guard.close(remove_cwd=True, cleanup_end=self.hard_end)
                finally:
                    self.mark(trust_handles_closed=guard.closed, cwd_closed=guard.created_directory is None,
                              ownership_lost=self.facts["ownership_lost"] or guard.lost)
            else:
                shutil.rmtree(self.plan["cwd"])
                self.mark(cwd_closed=True)
            remaining(self.hard_end, self.clock)
        except BaseException as error:
            if self.authority is not None and getattr(self.authority, "lost", False):
                self.mark(ownership_lost=True)
            self.fail(error.code if isinstance(error, (TransportFailure, worker_support.WorkerError)) else "cleanup_os")
        self.facts["completion_confirmed"] = settlement(self.facts)
        if not self.facts["completion_confirmed"]:
            self.facts["primary_error"] = self.facts["error"]
            self.facts["error"] = "cleanup_unconfirmed"
        return dict(self.facts)


class JevTransport:
    def __init__(self, *, clock=time.monotonic, plan_factory=prepare_plan, lease_factory=OwnedLease):
        self.clock, self.plan_factory, self.lease_factory = clock, plan_factory, lease_factory
        self.run_started = clock()
        self.run_end = self.run_started + RUN_SECONDS
        self.calls = 0
        self.lock = threading.Lock()
        self.active = None
        self.retained = []
        self.poisoned = False
        self.last_receipt = None

    def cancel(self):
        with self.lock:
            lease = self.active
        if lease is not None:
            lease.cancel()

    def send(self, wire, *, token, timeout=REQUEST_SECONDS, endpoint=contract.ENDPOINT, request_end=None):
        started = self.clock()
        with self.lock:
            require(not self.poisoned and self.active is None, "lease_unavailable")
            require(self.calls < MAX_CALLS, "request_count")
            try:
                operation_end, hard_end = request_deadlines(started, self.run_end, timeout, request_end=request_end)
            except TransportFailure as error:
                receipt = {"error": error.code, "phase": "admission", "process_admitted": False,
                           "completion_confirmed": False, "request_started": started,
                           "run_started": self.run_started, "run_end": self.run_end,
                           "request_end_supplied": request_end is not None, "calls_used": self.calls}
                try:
                    if type(request_end) in (int, float) and math.isfinite(request_end):
                        receipt["request_end"] = request_end
                except OverflowError:
                    pass
                self.last_receipt = receipt
                raise TransportFailure(error.code, receipt) from error
            require(endpoint == contract.ENDPOINT, "endpoint_policy")
            contract.validate_wire(wire)
            credential = token_bytes(token)
            self.calls += 1
            try:
                plan = self.plan_factory(wire, operation_end, hard_end, self.clock)
            except BaseException as error:
                code = error.code if isinstance(error, (TransportFailure, contract.ContractError)) else "preparation_os"
                receipt = {**getattr(error, "receipt", {}), "error": code, "phase": "preparation", "process_admitted": False,
                           "request_started": started, "operation_end": operation_end, "hard_end": hard_end,
                           "run_end": self.run_end, "completion_confirmed": False, "calls_used": self.calls}
                self.last_receipt, self.poisoned = receipt, True
                owner = getattr(error, "owner", None)
                if owner is not None:
                    self.retained.append(owner)
                raise TransportFailure(code, receipt, owner=owner) from error
            lease = self.lease_factory(plan, operation_end, hard_end, clock=self.clock)
            self.active = lease
            self.retained.append(lease)
        failure = None
        try:
            lease.start()
            lease.await_state(lambda facts: facts["start_finished"] or facts["error"], operation_end)
            require(not lease.facts["error"], lease.facts["error"] or "creation_os")
            lease.admit(wire, credential)
            lease.await_state(lambda facts: lease.streams_and_exit_done(facts) or facts["error"], operation_end)
            require(not lease.facts["error"], lease.facts["error"] or "helper_failure")
        except BaseException as error:
            failure = error.code if isinstance(error, (TransportFailure, contract.ContractError)) else "controller_os"
            lease.cancel(failure)
        receipt = lease.finish()
        receipt.update(request_started=started, run_started=self.run_started, run_end=self.run_end,
                       calls_used=self.calls, wire_sha256=contract.sha256(wire),
                       interpreter_sha256=plan["header"]["interpreter_sha256"],
                       worker_sha256=plan["header"]["worker_sha256"])
        with self.lock:
            self.last_receipt = receipt
            self.poisoned = not receipt["completion_confirmed"]
            if not self.poisoned:
                self.active = None
        if self.poisoned:
            raise TransportFailure("cleanup_unconfirmed", receipt)
        if failure or receipt["error"] or lease.cancelled:
            raise TransportFailure(failure or receipt["error"] or "cancelled", receipt)
        try:
            payload = contract.decode(lease.stdout, MAX_CAPTURE)
            require(type(payload) is dict and payload.get("version") == 1, "helper_receipt_schema")
            receipt["helper"] = {key: value for key, value in payload.items() if key != "answers"}
            if "windows_guard" in plan and payload.get("windows_trust_closed") is not True:
                receipt.update(process_completion_confirmed=receipt["completion_confirmed"],
                               completion_confirmed=False, helper_trust_unconfirmed=True)
                with self.lock:
                    self.poisoned, self.active = True, lease
                code = payload.get("error", "helper_trust_unconfirmed") if payload.get("status") == "failed" else \
                    "helper_trust_unconfirmed"
                raise TransportFailure(code, receipt)
            require(payload.get("credential_channel_closed") is True, "helper_channel_unconfirmed")
            if payload.get("status") != "completed":
                raise TransportFailure(payload.get("error", "helper_failure"), receipt)
            require(receipt["returncode"] == 0 and payload.get("wire_sha256") == receipt["wire_sha256"] and
                    payload.get("socket_closed") is True, "helper_completion")
            tls = payload.get("tls")
            require(type(tls) is dict and tls.get("certificate_verified") is True and
                    tls.get("hostname_verified") is True and type(tls.get("root_count")) is int and
                    tls["root_count"] > 0 and type(tls.get("root_der_set_sha256")) is str and
                    len(tls["root_der_set_sha256"]) == 64, "helper_tls_receipt")
            contract.validate_answers({"answers": payload.get("answers")}, contract.validate_wire(wire)["questions"])
            remaining(hard_end, self.clock)
            return TransportResult({"answers": payload["answers"]}, receipt)
        except (TransportFailure, contract.ContractError) as error:
            receipt["error"] = error.code
            raise TransportFailure(error.code, receipt) from error
