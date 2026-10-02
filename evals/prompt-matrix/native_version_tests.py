"""Dedicated native-version resource controls; all execution dependencies are fake."""

import unittest
import copy
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
from unittest.mock import patch

from native_assembly import qualify_receipt
from native_layout import FixtureError
from deadline_io import Deadline
from native_version_runner import (CAP, HARD_SECONDS, CLEANUP_SECONDS, SCOPE, digest, encoded, environment_for,
                                   execute_request, request_for, validate_request, validate_receipt)
from native_version_runner import input_digest, validate_sidecar, write_exclusive
from native_version_platform import ChildAuthority, UnixOwner, WindowsOwner, WinAPI, WinRead, unix_preflight


class Clock:
    def __init__(self, value=0):
        self.value = value

    def __call__(self):
        return self.value


class FakeThread:
    def __init__(self, target, daemon):
        self.target, self.alive, self.joins = target, False, []
        if daemon:
            raise AssertionError("Native owner must remain explicit")

    def start(self):
        self.alive = True
        self.target()
        self.alive = False

    def join(self, timeout):
        self.joins.append(timeout)

    def is_alive(self):
        return self.alive


class FakeOwner:
    kind = "unix_waitid_sole_reaper"

    def __init__(self, clock, events=None, code=0, late=False, failure=False, resources=None):
        self.clock = clock
        self.events = list(events if events is not None else [("stdout", b"opaque 1.2.3\n"),
                          ("stderr", b""), ("stdout", b""), ("exit", code)])
        self.authority = ChildAuthority(clock)
        self.late, self.failure = late, failure
        self.finished, self.closed, self.starts = False, False, 0
        self.signals, self.ends = [], []
        self.resources = resources or {}

    def start(self):
        self.starts += 1
        if self.failure:
            raise OSError(5, "supplied creation failure")
        if self.late:
            self.clock.value = 11
        cancelled = self.authority.capture("exact-child")
        if cancelled or self.late:
            self.cancel()

    def cancel(self):
        return self.authority.cancel(self.signals.append)

    def next(self, deadline, capacities):
        self.ends.append(deadline.end)
        if not self.events:
            self.clock.value = deadline.end
            deadline.remaining()
        name, value = self.events.pop(0)
        if name == "exit":
            self.authority.observe(value)
            self.authority.collect(lambda child: value)
            return [("wake", None)]
        return [(name, value)]

    def finish(self, hard, cancel=False):
        self.ends.append(hard.end)
        if cancel:
            self.cancel()
        hard.remaining()
        self.finished, self.closed = True, True

    def facts(self):
        return self.authority.facts() | {"kind": self.kind, "reaped": self.authority.collected,
                "waiter_finished": self.finished, "waiter_joined": self.finished,
                "capture_completed": self.closed, "handles_closed": self.closed,
                "waiter_joined_at": self.clock() if self.finished else None,
                "handles_closed_at": self.clock() if self.closed else None} | self.resources


def fixture_request(root, clock, platform="linux"):
    directory = root / "native/claude/1.2.3"
    directory.mkdir(parents=True)
    executable = directory / "opaque-native"
    executable.write_bytes(b"opaque fake input; never executed")
    home = directory / "qualification-home"
    home.mkdir()
    (directory / "qualification").mkdir()
    environment = environment_for(root, home, "linux", "claude")
    manifest = {"provider": "claude", "version": "1.2.3", "platform": platform, "arch": "x64",
                "executable_sha256": digest(executable.read_bytes()), "version_receipt": {"status": "NOT RUN"}}
    return request_for(root, directory, manifest, executable, {"instance": "opaque", "port": 7794},
                       environment, clock()), environment


def qualification_fixture():
    with tempfile.TemporaryDirectory() as temporary:
        clock = Clock()
        request, environment = fixture_request(Path(temporary).resolve(), clock)
        owner = FakeOwner(clock)
        result = execute_request(request, environment, clock, lambda *args: owner)
        result.update(status="verified", inputs_revalidated=True)
        return result


class FakePipe:
    def __init__(self, descriptor):
        self.descriptor, self.closed = descriptor, False

    def fileno(self):
        return self.descriptor

    def close(self):
        self.closed = True


class FakeSelector:
    def __init__(self):
        self.rows, self.closed = {}, False

    def register(self, descriptor, mask, data):
        self.rows[descriptor] = data

    def unregister(self, descriptor):
        del self.rows[descriptor]

    def close(self):
        self.closed = True


class FakeUnix:
    P_PID, WEXITED, WNOWAIT, CLD_EXITED = 1, 2, 4, 5

    def __init__(self):
        self.calls = []

    def pipe(self):
        return 31, 32

    def set_blocking(self, descriptor, value):
        self.calls.append(("set_blocking", descriptor, value))

    def write(self, descriptor, data):
        self.calls.append(("wake", descriptor))
        return len(data)

    def close(self, descriptor):
        self.calls.append(("close", descriptor))

    def kill(self, pid, sig):
        self.calls.append(("kill", pid, sig))

    def waitid(self, mode, pid, flags):
        self.calls.append(("waitid", mode, pid, flags))
        return SimpleNamespace(si_pid=pid, si_code=self.CLD_EXITED, si_status=0)

    def waitpid(self, pid, flags):
        self.calls.append(("waitpid", pid, flags))
        return pid, 0

    def waitstatus_to_exitcode(self, status):
        return status


class FakeWin:
    def __init__(self):
        self.calls, self.next_handle = [], 100

    def event(self):
        self.next_handle += 1
        return self.next_handle

    def pipe(self):
        return self.event(), self.event(), self.event(), None

    def spawn(self, request, environment, slots, capture, auxiliary):
        self.calls.append(("CreateProcessW", request["argv"]))
        capture("captured-HANDLE")
        for slot in slots.values():
            self.close(slot.writer)
            slot.writer = None
        return "captured-HANDLE"

    def notify(self, handle):
        self.calls.append(("SetEvent", handle))

    def observe(self, handle):
        self.calls.append(("wait-process", handle))
        return 0

    def terminate(self, handle):
        self.calls.append(("TerminateProcess", handle))

    def close(self, handle):
        self.calls.append(("CloseHandle", handle))


class Box:
    def __init__(self):
        self.value = 0


def fake_overlap_api():
    api = WinAPI.__new__(WinAPI)
    api.c = SimpleNamespace(byref=lambda value: value, get_last_error=lambda: api.last_error)
    api.w = SimpleNamespace(DWORD=Box)
    api.calls, api.last_error = [], 996
    def cancel(handle, overlap):
        api.calls.append(("CancelIoEx", handle, overlap))
        return 1
    def complete(handle, overlap, count, wait):
        api.calls.append(("GetOverlappedResult", handle, overlap, wait))
        return 0
    api.kernel = SimpleNamespace(CancelIoEx=cancel, GetOverlappedResult=complete,
                                 CloseHandle=lambda handle: api.calls.append(("CloseHandle", handle)) or 1)
    return api


class NativeVersionControls(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.clock = Clock()
        self.request, self.environment = fixture_request(Path(temporary.name).resolve(), self.clock)

    def run_fake(self, **options):
        self.owner = FakeOwner(self.clock, **options)
        result = execute_request(self.request, self.environment, self.clock, lambda *args: self.owner)
        result.update(status="verified", inputs_revalidated=True)
        return result

    def reject(self, receipt):
        with self.assertRaisesRegex(FixtureError, "unqualified"):
            qualify_receipt(receipt, self.request["version"], self.request["executable"],
                            self.request["executable_sha256"])

    def test_normal_exact_version_and_resources(self):
        receipt = self.run_fake()
        qualify_receipt(receipt, "1.2.3", self.request["executable"], self.request["executable_sha256"])
        self.assertTrue(receipt["resources"]["exit_collected"])
        self.assertTrue(receipt["resources"]["handles_closed"])
        self.assertEqual(self.owner.starts, 1)

    def test_wrong_version_refused(self):
        self.reject(self.run_fake(events=[("stdout", b"opaque 4.5.6"), ("stdout", b""),
                                         ("stderr", b""), ("exit", 0)]))

    def test_nonzero_exit_refused(self):
        self.reject(self.run_fake(code=9))

    def overflow_events(self, stream):
        return [(stream, b"x" * 4096)] * 8 + [(stream, b"x")]

    def test_stdout_cap_cancels(self):
        receipt = self.run_fake(events=self.overflow_events("stdout"))
        self.assertEqual(receipt["streams"]["stdout"]["bytes"], CAP)
        self.assertTrue(receipt["streams"]["stdout"]["overflow"])
        self.assertEqual(self.owner.signals, ["exact-child"])
        self.reject(receipt)

    def test_stderr_cap_cancels(self):
        receipt = self.run_fake(events=self.overflow_events("stderr"))
        self.assertTrue(receipt["streams"]["stderr"]["overflow"])
        self.assertEqual(self.owner.signals, ["exact-child"])
        self.reject(receipt)

    def test_read_error_is_not_eof(self):
        receipt = self.run_fake(events=[("stdout", {"type": "OSError", "errno": 5})])
        self.assertFalse(receipt["streams"]["stdout"]["eof"])
        self.assertEqual(receipt["streams"]["stdout"]["error"]["errno"], 5)
        self.reject(receipt)

    def test_missing_stdout_eof_refused(self):
        self.reject(self.run_fake(events=[("stdout", b"1.2.3"), ("stderr", b""), ("exit", 0)]))

    def test_missing_stderr_eof_refused(self):
        self.reject(self.run_fake(events=[("stdout", b"1.2.3"), ("stdout", b""), ("exit", 0)]))

    def test_retained_descendant_pipe_refused(self):
        receipt = self.run_fake(events=[("exit", 0), ("stdout", b"1.2.3"), ("stdout", b"")])
        self.assertTrue(receipt["resources"]["exit_collected"])
        self.assertFalse(receipt["streams"]["stderr"]["eof"])
        self.reject(receipt)

    def test_direct_success_makes_no_tree_claim(self):
        receipt = self.run_fake()
        self.assertEqual(receipt["scope"], SCOPE)
        self.assertEqual(receipt["descendants"], "not_proven")
        self.assertNotIn("tree_stopped", receipt["resources"])
        validate_receipt(receipt, "1.2.3", self.request["executable"], self.request["executable_sha256"])

    def test_changed_executable_and_input_hash_refused(self):
        receipt = self.run_fake()
        changed = copy.deepcopy(receipt)
        changed["request"]["executable_sha256"] = "a" * 64
        self.reject(changed)
        changed = copy.deepcopy(receipt)
        changed["request"]["assembly_input_sha256"] = "b" * 64
        self.reject(changed)
        directory = Path(self.request["directory"])
        manifest = {"provider": "claude", "version": "1.2.3", "platform": "linux", "arch": "x64",
                    "executable_sha256": self.request["executable_sha256"], "version_receipt": {"status": "NOT RUN"}}
        hard = Deadline(10, self.clock)
        sidecar = directory / "qualification"
        write_exclusive(sidecar / "assembly-before.json", manifest, hard)
        write_exclusive(sidecar / "request.json", self.request, hard)
        receipt_hash = write_exclusive(sidecar / "receipt.json", receipt, hard)
        completion = {"receipt_sha256": receipt_hash, "request_sha256": receipt["request_sha256"],
                      "assembly_input_sha256": input_digest(manifest), "recorded_at": 0}
        completion_hash = write_exclusive(sidecar / "completion.json", completion, hard)
        reference = {"status": "verified", "sidecar": "qualification/receipt.json", "sha256": receipt_hash,
                     "completion": "qualification/completion.json", "completion_sha256": completion_hash,
                     "finalized_at": 0}
        self.assertEqual(validate_sidecar(directory, reference, manifest), receipt)
        changed_manifest = manifest | {"arch": "arm64"}
        with self.assertRaises(FixtureError):
            validate_sidecar(directory, reference, changed_manifest)
        (sidecar / "request.json").write_text(json.dumps(self.request | {"port": 7795}))
        with self.assertRaisesRegex(FixtureError, "retained inputs"):
            validate_sidecar(directory, reference, manifest)

    def test_foreign_argv_cwd_home_refused(self):
        receipt = self.run_fake()
        for key, value in (("argv", ["owner-native", "--version"]), ("cwd", self.request["root"]),
                           ("home", self.request["root"])):
            changed = copy.deepcopy(receipt)
            changed["request"][key] = value
            self.reject(changed)

    def test_minimal_windows_environment(self):
        system = Path(self.request["root"]) / "trusted-system"
        (system / "System32").mkdir(parents=True)
        (system / "System32/kernel32.dll").write_bytes(b"opaque fake trust input")
        with patch.dict("os.environ", {"PATH": "owner", "COMSPEC": "owner", "PATHEXT": "owner",
                                      "CODETERM_API_TOKEN": "owner", "HTTPS_PROXY": "owner"}, clear=True):
            environment = environment_for(Path(self.request["root"]), Path(self.request["home"]),
                                          "win32", "claude", system_root=system)
        self.assertEqual(environment["SystemRoot"], str(system))
        self.assertEqual(environment["PATH"], "")
        for key in ("COMSPEC", "PATHEXT", "CODETERM_API_TOKEN", "HTTPS_PROXY"):
            self.assertNotIn(key, environment)
        for key in ("HOME", "TEMP", "APPDATA", "CODEX_HOME", "npm_config_cache"):
            self.assertTrue(Path(environment[key]).is_relative_to(Path(self.request["home"])))

    def test_pre_spawn_deadline_refuses_creation(self):
        self.clock.value = 8
        self.reject(self.run_fake())
        self.assertEqual(self.owner.starts, 0)

    def test_creation_failure_is_not_captured(self):
        receipt = self.run_fake(failure=True)
        self.assertFalse(receipt["resources"]["captured"])
        self.assertEqual(self.owner.signals, [])
        self.reject(receipt)

    def test_late_child_is_cancelled(self):
        receipt = self.run_fake(late=True)
        self.assertEqual(self.owner.signals, ["exact-child"])
        self.assertFalse(receipt["resources"]["waiter_finished"])
        self.reject(receipt)
        self.clock.value = 0
        api = FakeUnix()
        child = SimpleNamespace(pid=77, stdout=FakePipe(81), stderr=FakePipe(82), returncode=None)
        def late_factory(*args, **kwargs):
            self.clock.value = 11
            return child
        owner = UnixOwner(self.request, self.environment, self.clock, late_factory, api, FakeSelector, FakeThread)
        with patch("native_version_platform.signal.SIGKILL", 9, create=True):
            owner.start()
        with self.assertRaises(FixtureError):
            owner.finish(Deadline(10, self.clock))
        self.assertEqual(len([row for row in api.calls if row[0] == "kill"]), 1)

    def test_zero_wait_budget_still_cancels_unconfirmed(self):
        owner = FakeOwner(self.clock)
        owner.start()
        self.clock.value = 10
        with self.assertRaises(FixtureError):
            owner.finish(Deadline(10, self.clock), cancel=True)
        self.assertEqual(owner.signals, ["exact-child"])
        self.assertFalse(owner.facts()["exit_observed"])
        api = FakeWin()
        captured = WindowsOwner(self.request, self.environment, self.clock, api, FakeThread)
        captured.capture("late-HANDLE")
        captured.started = True
        with self.assertRaises(FixtureError):
            captured.finish(Deadline(10, self.clock), cancel=True)
        self.assertIn(("TerminateProcess", "late-HANDLE"), api.calls)
        self.assertFalse(captured.facts()["handles_closed"])

    def test_cancel_reap_interleavings_preserve_authority(self):
        calls = []
        authority = ChildAuthority()
        authority.cancel(calls.append)
        self.assertTrue(authority.capture("child"))
        authority.cancel(calls.append)
        authority.observe(0)
        authority.collect(lambda child: calls.append(("reap", child)) or 0)
        authority.cancel(calls.append)
        self.assertEqual(calls, ["child", ("reap", "child")])
        lost = ChildAuthority()
        lost.capture("lost-child")
        lost.lose(OSError(10, "supplied ECHILD"))
        lost.cancel(calls.append)
        self.assertEqual(len(calls), 2)
        racing = ChildAuthority()
        racing.capture("racing")
        def signal_then_observe(child):
            racing.observe(0)
            with self.assertRaises(FixtureError):
                racing.collect(lambda owned: 0)
        racing.cancel(signal_then_observe)
        racing.collect(lambda owned: 0)
        self.assertTrue(racing.facts()["exit_collected"])

    def test_unix_wnowait_and_sole_final_reap(self):
        api = FakeUnix()
        child = SimpleNamespace(pid=77, stdout=FakePipe(81), stderr=FakePipe(82), returncode=None)
        launches = []
        def factory(argv, **kwargs):
            launches.append((argv, kwargs))
            return child
        unix_preflight("darwin", (3, 13), api, handler=0)
        with self.assertRaises(FixtureError):
            unix_preflight("darwin", (3, 12), api, handler=0)
        owner = UnixOwner(self.request, self.environment, self.clock, factory, api, FakeSelector, FakeThread)
        owner.start()
        owner.finish(Deadline(10, self.clock))
        owner.cancel()
        waits = [row for row in api.calls if row[0] in ("waitid", "waitpid", "kill")]
        self.assertEqual(waits, [("waitid", api.P_PID, 77, api.WEXITED | api.WNOWAIT), ("waitpid", 77, 0)])
        self.assertEqual(child.returncode, 0)
        self.assertTrue(owner.facts()["waiter_joined"])
        self.assertTrue(owner.facts()["handles_closed"])
        self.assertFalse(launches[0][1]["shell"])
        self.assertEqual(launches[0][1]["cwd"], self.request["cwd"])

    def test_windows_cancel_uses_captured_handle(self):
        api = FakeWin()
        owner = WindowsOwner(self.request, self.environment, self.clock, api, FakeThread)
        owner.cancel()
        owner.start()
        self.assertFalse(owner.facts()["captured"])
        other = WindowsOwner(self.request, self.environment, self.clock, api, FakeThread)
        other.capture("exact-HANDLE")
        other.cancel()
        self.assertEqual([row for row in api.calls if row[0] == "TerminateProcess"],
                         [("TerminateProcess", "exact-HANDLE")])
        self.assertFalse(any("PID" in row[0] or "OpenProcess" in row[0] for row in api.calls))
        normal = WindowsOwner(self.request, self.environment, self.clock, api, FakeThread)
        normal.start()
        normal.finish(Deadline(10, self.clock))
        self.assertTrue(normal.facts()["handles_closed"])
        exit_index = api.calls.index(("wait-process", "captured-HANDLE"))
        close_index = api.calls.index(("CloseHandle", "captured-HANDLE"))
        self.assertLess(exit_index, close_index)
        public = WinAPI.__new__(WinAPI)
        public.c = SimpleNamespace(get_last_error=lambda: 0)
        calls = []
        public.kernel = SimpleNamespace(TerminateProcess=lambda handle, code: calls.append((handle, code)) or 1)
        authority = ChildAuthority(self.clock)
        authority.capture("public-captured-HANDLE")
        authority.cancel(public.terminate)
        self.assertEqual(calls, [("public-captured-HANDLE", 1)])

    def test_cancelioex_is_not_completion(self):
        api = fake_overlap_api()
        slot = WinRead.__new__(WinRead)
        slot.reader, slot.writer, slot.event = "read-HANDLE", None, "event-HANDLE"
        slot.connection, slot.overlap, slot.buffer = None, object(), SimpleNamespace(raw=b"opaque")
        slot.pending, slot.closed, slot.cancel_requested = True, False, False
        slot.cancel(api)
        self.assertTrue(slot.pending)
        self.assertEqual(api.read_finish(slot), ("pending", None))
        with self.assertRaisesRegex(FixtureError, "completion unconfirmed"):
            slot.close(api)
        self.assertIsNotNone(slot.buffer)
        api.last_error = 995
        self.assertEqual(api.read_finish(slot), ("cancelled", None))
        slot.close(api)
        self.assertTrue(slot.closed)
        names = [row[0] for row in api.calls]
        self.assertEqual(names, ["CancelIoEx", "GetOverlappedResult", "GetOverlappedResult", "CloseHandle", "CloseHandle"])

    def test_waiter_and_handle_failures_refused(self):
        for key in ("waiter_finished", "waiter_joined", "handles_closed", "capture_completed"):
            self.reject(self.run_fake(resources={key: False}))
        api = fake_overlap_api()
        api.kernel.CloseHandle = lambda handle: 0
        slot = WinRead.__new__(WinRead)
        slot.reader, slot.writer, slot.event = "retained-HANDLE", None, "retained-event"
        slot.buffer, slot.overlap, slot.connection = object(), object(), None
        slot.pending, slot.closed = False, False
        with self.assertRaises(OSError):
            slot.close(api)
        self.assertFalse(slot.closed)
        self.assertEqual(slot.reader, "retained-HANDLE")
        self.assertIsNotNone(slot.buffer)

    def test_original_deadline_never_renews(self):
        receipt = self.run_fake()
        self.assertEqual(set(self.owner.ends), {8, 10})
        self.assertEqual(self.request["hard_deadline"] - self.request["started_at"], HARD_SECONDS)
        self.assertEqual(self.request["hard_deadline"] - self.request["operation_deadline"], CLEANUP_SECONDS)
        changed = copy.deepcopy(self.request)
        changed["hard_deadline"] = 15
        with self.assertRaisesRegex(FixtureError, "original deadline"):
            validate_request(changed)
        receipt["completed_at"] = 10
        self.reject(receipt)

    def test_legacy_receipt_is_refused(self):
        receipt = {"status": "verified", "version": "supplied-version", "executable_sha256": "supplied-sha",
                   "exit_code": 0, "argv": ["supplied-executable", "--version"]}
        with self.assertRaisesRegex(FixtureError, "unqualified"):
            qualify_receipt(receipt, "supplied-version", "supplied-executable", "supplied-sha")


if __name__ == "__main__":
    unittest.main()
