"""Deterministic IO/deadline and tar controls; no real socket, thread or network."""

import base64
import errno
import hashlib
import io
import json
from email.message import Message
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import busy_fixture
import acquisition_worker as worker
import acquisition_transport as parent
from deadline_io import Cancelled, Deadline, SocketIO, completed_worker
from fixture_tests import NativeControls
from native_archive import acquire, extract
from native_layout import FixtureError


class Clock:
    def __init__(self):
        self.now = 0

    def __call__(self):
        return self.now

    def advance(self, seconds):
        self.now += seconds


class Flag:
    def __init__(self, value=False):
        self.value, self.waits = value, []

    def is_set(self):
        return self.value

    def set(self):
        self.value = True

    def wait(self, timeout):
        self.waits.append(timeout)
        return self.value


class FakeSocket:
    def __init__(self, clock, reads=(), sends=(), connect=(0, 0), accepted=None):
        self.clock, self.reads, self.sends = clock, list(reads), list(sends)
        self.connect_result, self.accepted = connect, accepted
        self.recv_limits, self.send_chunks = [], []
        self.closed = False

    def setblocking(self, value):
        self.blocking = value

    def recv(self, limit):
        self.recv_limits.append(limit)
        result, advance = self.reads.pop(0)
        self.clock.advance(advance)
        if isinstance(result, Exception):
            raise result
        return result

    def send(self, chunk):
        self.send_chunks.append(chunk)
        result, advance = self.sends.pop(0) if self.sends else (len(chunk), 0)
        self.clock.advance(advance)
        if isinstance(result, Exception):
            raise result
        return result

    def connect_ex(self, address):
        result, advance = self.connect_result
        self.clock.advance(advance)
        return result

    def getsockopt(self, *_):
        return 0

    def accept(self):
        connection, advance = self.accepted
        self.clock.advance(advance)
        return connection, ("127.0.0.1", 1234)

    def shutdown(self, *_):
        self.shut_down = True

    def close(self):
        self.closed = True


class Wait:
    def __init__(self, clock, stream, steps, cancel=None, wake=None):
        self.clock, self.stream, self.steps = clock, stream, list(steps)
        self.cancel, self.wake, self.timeouts = cancel, wake, []

    def __call__(self, readers, writers, exceptional, timeout):
        self.timeouts.append(timeout)
        advance, mode = self.steps.pop(0)
        self.clock.advance(advance)
        if mode == "cancel":
            self.cancel.set()
            return [self.wake], [], []
        if mode == "none":
            return [], [], []
        return ([self.stream], [], []) if mode == "read" else ([], [self.stream], [])


class FakeThread:
    def __init__(self, clock, alive=False, remain_alive=False, advance=0):
        self.clock, self.alive = clock, alive
        self.remain_alive, self.advance = remain_alive, advance
        self.joins = []

    def join(self, timeout):
        self.joins.append(timeout)
        self.clock.advance(self.advance)
        self.alive = self.remain_alive

    def is_alive(self):
        return self.alive


class DeadlineControls(unittest.TestCase):
    def setUp(self):
        self.clock = Clock()
        self.deadline = Deadline(10, self.clock)

    def test_partial_frame_control(self):
        stream = FakeSocket(self.clock, [(BlockingIOError(), 0), (b"a", 0),
                                        (BlockingIOError(), 0), (b"b\n", 0)])
        wait = Wait(self.clock, stream, [(2, "read"), (3, "read")])
        self.assertEqual(SocketIO(stream, self.deadline, wait=wait).frame(), b"ab\n")
        self.assertEqual(wait.timeouts, [10, 8])
        self.assertTrue(all(limit <= 256 for limit in stream.recv_limits))

    def test_partial_arrivals_cannot_renew_deadline(self):
        stream = FakeSocket(self.clock, [(BlockingIOError(), 0), (b"a", 0), (BlockingIOError(), 0),
                                        (b"b", 0), (BlockingIOError(), 0), (b"\n", 0)])
        wait = Wait(self.clock, stream, [(3, "read"), (3, "read"), (5, "read")])
        with self.assertRaisesRegex(FixtureError, "Absolute IO deadline"):
            SocketIO(stream, self.deadline, wait=wait).frame()
        self.assertEqual(wait.timeouts, [10, 7, 4])
        self.assertEqual(len(stream.reads), 1)

    def test_late_complete_frame_is_refused(self):
        stream = FakeSocket(self.clock, [(b"complete\n", 11)])
        with self.assertRaisesRegex(FixtureError, "deadline"):
            SocketIO(stream, self.deadline).frame()

    def test_frame_byte_limit_is_refused(self):
        stream = FakeSocket(self.clock, [(b"ab", 0), (b"cd", 0)])
        with self.assertRaisesRegex(FixtureError, "byte limit"):
            SocketIO(stream, self.deadline).frame(limit=3)

    def test_eof_is_not_frame_completion(self):
        with self.assertRaisesRegex(FixtureError, "closed before completion"):
            SocketIO(FakeSocket(self.clock, [(b"", 0)]), self.deadline).frame()

    def test_trailing_frame_bytes_are_refused(self):
        with self.assertRaisesRegex(FixtureError, "trailing bytes"):
            SocketIO(FakeSocket(self.clock, [(b"a\nb", 0)]), self.deadline).frame()

    def test_cancellation_wakes_partial_read(self):
        stream = FakeSocket(self.clock, [(BlockingIOError(), 0)])
        cancel, wake = Flag(), object()
        wait = Wait(self.clock, stream, [(1, "cancel")], cancel, wake)
        with self.assertRaises(Cancelled):
            SocketIO(stream, self.deadline, cancel, wake, wait).frame()

    def test_empty_wait_cannot_mean_readiness(self):
        stream = FakeSocket(self.clock, [(BlockingIOError(), 0)])
        with self.assertRaisesRegex(FixtureError, "without readiness"):
            SocketIO(stream, self.deadline, wait=Wait(self.clock, stream, [(0, "none")])).frame()

    def test_release_cannot_get_a_new_budget(self):
        self.clock.advance(10)
        stream = FakeSocket(self.clock)
        with self.assertRaisesRegex(FixtureError, "deadline"):
            SocketIO(stream, self.deadline).send(b"release\n")
        self.assertEqual(stream.send_chunks, [])

    def test_partial_send_cannot_renew_deadline(self):
        stream = FakeSocket(self.clock, sends=[(1, 4), (1, 4), (1, 4)])
        with self.assertRaisesRegex(FixtureError, "deadline"):
            SocketIO(stream, self.deadline).send(b"ab\n")

    def test_connect_and_read_share_deadline(self):
        stream = FakeSocket(self.clock, reads=[(BlockingIOError(), 0), (b"a\n", 0)],
                            connect=(errno.EINPROGRESS, 3))
        wait = Wait(self.clock, stream, [(4, "write"), (4, "read")])
        channel = SocketIO(stream, self.deadline, wait=wait)
        channel.connect(("127.0.0.1", 1234))
        with self.assertRaisesRegex(FixtureError, "deadline"):
            channel.frame()
        self.assertEqual(wait.timeouts, [7, 3])

    def test_late_accept_closes_unassigned_connection(self):
        accepted = FakeSocket(self.clock)
        stream = FakeSocket(self.clock, accepted=(accepted, 11))
        with self.assertRaisesRegex(FixtureError, "deadline"):
            SocketIO(stream, self.deadline).accept()
        self.assertTrue(accepted.closed)

    def test_finished_flag_and_thread_exit_control(self):
        thread = FakeThread(self.clock, alive=True)
        completed_worker(thread, Flag(True), self.deadline)
        self.assertFalse(thread.is_alive())

    def test_closed_reader_is_not_worker_completion(self):
        thread = FakeThread(self.clock, alive=True)
        with self.assertRaisesRegex(FixtureError, "completion unconfirmed"):
            completed_worker(thread, Flag(False), self.deadline)
        self.assertEqual(thread.joins, [])

    def test_finished_flag_is_not_thread_exit(self):
        thread = FakeThread(self.clock, alive=True, remain_alive=True)
        with self.assertRaisesRegex(FixtureError, "thread termination"):
            completed_worker(thread, Flag(True), self.deadline)

    def test_late_join_cannot_mean_cleanup_success(self):
        thread = FakeThread(self.clock, alive=True, advance=11)
        with self.assertRaisesRegex(FixtureError, "deadline"):
            completed_worker(thread, Flag(True), self.deadline)

    def test_busy_close_refusal_keeps_closed_false(self):
        gate = object.__new__(busy_fixture.BusyGate)
        gate.closed, gate.cancel = False, Flag()
        gate.deadline, gate.finished = self.deadline, Flag(False)
        gate.thread = FakeThread(self.clock, alive=True)
        gate.connection = FakeSocket(self.clock)
        gate.socket, gate.wake_reader, gate.wake_writer = (FakeSocket(self.clock) for _ in range(3))
        with self.assertRaisesRegex(FixtureError, "completion unconfirmed"):
            gate.close()
        self.assertFalse(gate.closed)
        self.assertTrue(gate.cancel.is_set())
        self.assertTrue(gate.connection.closed and gate.socket.closed and gate.wake_reader.closed and gate.wake_writer.closed)

    def test_main_connect_read_has_no_separate_150s_grants(self):
        nonce = "supplied-nonce"
        stream = FakeSocket(self.clock, reads=[(BlockingIOError(), 0),
                            (busy_fixture.frame({"event": "release", "nonce": nonce}), 0)], connect=(0, 3))
        wait = Wait(self.clock, stream, [(6, "read")])
        factory = lambda stream, deadline: SocketIO(stream, deadline, wait=wait)
        with patch.object(busy_fixture.socket, "socket", return_value=stream), \
             patch.object(busy_fixture, "Deadline", side_effect=lambda end: Deadline(end, self.clock)), \
             patch.object(busy_fixture, "SocketIO", side_effect=factory), \
             patch("sys.argv", ["busy_fixture.py", "--port", "1234", "--nonce", nonce, "--deadline", "10"]), \
             patch("sys.stdout", new_callable=io.StringIO) as output:
            with self.assertRaisesRegex(FixtureError, "deadline"):
                busy_fixture.main()
            self.assertEqual(output.getvalue(), "")
        self.assertEqual(wait.timeouts, [5])
        self.assertTrue(stream.closed)


class ArchiveCollisionControls(unittest.TestCase):
    def tar(self, entries):
        return NativeControls.archive(self, entries)

    def reject(self, entries, predicate):
        archive, destination = self.tar(entries)
        with self.assertRaisesRegex(FixtureError, predicate):
            extract(archive, destination, float("inf"))

    def test_implicit_parent_case_collision(self):
        self.reject([("package/A/x", tarfile.REGTYPE, b"x"), ("package/a/y", tarfile.REGTYPE, b"y")], "case-colliding")

    def test_nested_implicit_parent_case_collision(self):
        self.reject([("package/A/B/x", tarfile.REGTYPE, b"x"), ("package/A/b/y", tarfile.REGTYPE, b"y")], "case-colliding")

    def test_implicit_parent_then_different_explicit_spelling(self):
        self.reject([("package/A/x", tarfile.REGTYPE, b"x"), ("package/a", tarfile.DIRTYPE, "")], "case-colliding")

    def test_explicit_parent_then_different_implicit_spelling(self):
        self.reject([("package/A", tarfile.DIRTYPE, ""), ("package/a/y", tarfile.REGTYPE, b"y")], "case-colliding")

    def test_same_spelled_implicit_directory_can_be_declared(self):
        archive, destination = self.tar([("package/A/x", tarfile.REGTYPE, b"x"),
                                        ("package/A", tarfile.DIRTYPE, ""), ("package/A/y", tarfile.REGTYPE, b"y")])
        receipt = extract(archive, destination, float("inf"))
        self.assertEqual([row["path"] for row in receipt["files"]], ["A/x", "A/y"])

    def test_file_cannot_be_an_ancestor(self):
        self.reject([("package/A", tarfile.REGTYPE, b"x"), ("package/A/y", tarfile.REGTYPE, b"y")], "file/directory")

    def test_implicit_directory_cannot_become_file(self):
        self.reject([("package/A/x", tarfile.REGTYPE, b"x"), ("package/A", tarfile.REGTYPE, b"y")], "file/directory")

    def test_explicit_directory_cannot_become_file(self):
        self.reject([("package/A", tarfile.DIRTYPE, ""), ("package/A", tarfile.REGTYPE, b"y")], "file/directory")

    def test_same_spelled_explicit_directory_duplicate(self):
        self.reject([("package/A", tarfile.DIRTYPE, ""), ("package/A", tarfile.DIRTYPE, "")], "Duplicate")


class FakeDownload:
    def __init__(self, clock, reads, open_advance=0, completed=True, finish_advance=0):
        self.clock, self.reads = clock, list(reads)
        self.url = "https://registry.npmjs.org/opaque.tgz"
        self.content_length = None
        self.open_advance, self.completed, self.finish_advance = open_advance, completed, finish_advance
        self.finishes, self.read_limits = [], []

    def open(self, url, deadline):
        self.clock.advance(self.open_advance)
        return self

    def read_chunk(self, limit, deadline):
        self.read_limits.append(limit)
        value, advance = self.reads.pop(0)
        self.clock.advance(advance)
        return value

    def finish(self, cancel, deadline):
        self.finishes.append(cancel)
        self.clock.advance(self.finish_advance)
        return {"completed": self.completed}


class AcquisitionControls(unittest.TestCase):
    def setUp(self):
        self.clock = Clock()
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.path = Path(self.temporary.name) / "opaque.tgz"
        body = b"supplied opaque bytes"
        self.metadata = {"tarball": "https://registry.npmjs.org/opaque.tgz",
                         "integrity": "sha512-" + base64.b64encode(hashlib.sha512(body).digest()).decode(),
                         "tarball_sha256": hashlib.sha256(body).hexdigest()}
        self.body = body

    def test_default_acquisition_uses_owned_parent(self):
        supplied = {"sha256": "opaque-returned-receipt"}
        with patch("acquisition_transport.acquire_owned", return_value=supplied) as parent:
            self.assertEqual(acquire(self.metadata, self.path, 10, clock=self.clock), supplied)
        parent.assert_called_once_with(self.metadata, self.path, 10, clock=self.clock)
        self.assertFalse(self.path.exists())

    def test_fake_complete_download_control(self):
        transport = FakeDownload(self.clock, [(self.body, 1), (b"", 1)])
        self.assertEqual(acquire(self.metadata, self.path, 10, transport, self.clock)["sha256"], self.metadata["tarball_sha256"])
        self.assertEqual(transport.finishes, [False])
        self.assertTrue(all(limit == 64 * 1024 for limit in transport.read_limits))

    def test_partial_download_cannot_renew_absolute_deadline(self):
        transport = FakeDownload(self.clock, [(b"a", 3), (b"b", 3), (b"c", 3)])
        with self.assertRaisesRegex(FixtureError, "deadline"):
            acquire(self.metadata, self.path, 10, transport, self.clock)
        self.assertEqual(transport.finishes, [True])

    def test_late_header_open_is_cancelled(self):
        transport = FakeDownload(self.clock, [], open_advance=9)
        with self.assertRaisesRegex(FixtureError, "deadline"):
            acquire(self.metadata, self.path, 10, transport, self.clock)
        self.assertEqual(transport.finishes, [True])
        self.assertFalse(self.path.exists())

    def test_late_open_still_signals_cancel_after_hard_deadline(self):
        transport = FakeDownload(self.clock, [], open_advance=11)
        with self.assertRaisesRegex(FixtureError, "deadline"):
            acquire(self.metadata, self.path, 10, transport, self.clock)
        self.assertEqual(transport.finishes, [True])

    def test_eof_is_not_transport_completion(self):
        transport = FakeDownload(self.clock, [(self.body, 0), (b"", 0)], completed=False)
        with self.assertRaisesRegex(FixtureError, "completion unconfirmed"):
            acquire(self.metadata, self.path, 10, transport, self.clock)

    def test_late_transport_finish_cannot_qualify(self):
        transport = FakeDownload(self.clock, [(self.body, 0), (b"", 0)], finish_advance=11)
        with self.assertRaisesRegex(FixtureError, "deadline"):
            acquire(self.metadata, self.path, 10, transport, self.clock)

    def test_oversized_transport_chunk_is_cancelled(self):
        transport = FakeDownload(self.clock, [(b"x" * (64 * 1024 + 1), 0)])
        with self.assertRaisesRegex(FixtureError, "bounded chunk"):
            acquire(self.metadata, self.path, 10, transport, self.clock)
        self.assertEqual(transport.finishes, [True])

    def test_incomplete_declared_body_is_cancelled(self):
        transport = FakeDownload(self.clock, [(self.body, 0), (b"", 0)])
        transport.content_length = len(self.body) + 1
        with self.assertRaisesRegex(FixtureError, "declared completion"):
            acquire(self.metadata, self.path, 10, transport, self.clock)
        self.assertEqual(transport.finishes, [True])


class FakeProcess:
    def __init__(self, clock, code=0, wait_advance=0, wait_error=None, kill_error=None):
        self.clock, self.code, self.wait_advance = clock, code, wait_advance
        self.wait_error, self.kill_error = wait_error, kill_error
        self.waits, self.kills, self.handles_closed = 0, 0, False

    def wait(self):
        self.waits += 1
        self.clock.advance(self.wait_advance)
        if self.wait_error:
            raise self.wait_error
        return self.code

    def kill(self):
        self.kills += 1
        if self.kill_error:
            raise self.kill_error


class ImmediateThread:
    def __init__(self, target, daemon):
        self.target, self.daemon = target, daemon
        self.alive, self.joins = False, []

    def start(self):
        self.target()

    def join(self, timeout):
        self.joins.append(timeout)

    def is_alive(self):
        return self.alive


class DeferredThread(ImmediateThread):
    def start(self):
        self.alive = True


class ProcessControls(unittest.TestCase):
    def setUp(self):
        self.clock = Clock()
        self.hard = Deadline(10, self.clock)
        self.plan = {"operation": self.hard.reserve(2), "argv": ["opaque-trusted-python", "-I", "-S", "-B"],
                     "cwd": "opaque-owned-cwd", "env": {"HOME": "opaque-owned-home"}}
        self.process = FakeProcess(self.clock)
        self.calls = []
        self.closed = []

    def lifecycle(self, factory=None, close=None, thread=ImmediateThread):
        def create(argv, **kwargs):
            self.calls.append((argv, kwargs))
            return self.process
        def close_handles(process):
            process.handles_closed = True
            self.closed.append(process)
        return parent.AcquisitionProcess(self.plan, self.hard, factory or create, Flag, thread, close or close_handles)

    def test_owned_exit_and_handles_control(self):
        life = self.lifecycle()
        life.start()
        life.wait_exit()
        life.finish(cancel=False)
        self.assertEqual(self.process.waits, 1)
        self.assertEqual(self.closed, [self.process])
        self.assertEqual(self.process.kills, 0)
        self.assertTrue(life.snapshot()["exit_observed"] and life.snapshot()["handles_closed"])
        self.assertFalse(life.thread.is_alive())

    def test_launch_uses_no_shell_or_output_pipe(self):
        life = self.lifecycle()
        life.start()
        kwargs = self.calls[0][1]
        self.assertFalse(kwargs["shell"])
        self.assertTrue(kwargs["close_fds"])
        self.assertEqual((kwargs["stdin"], kwargs["stdout"], kwargs["stderr"]),
                         (parent.subprocess.DEVNULL,) * 3)
        self.assertEqual(self.calls[0][0], self.plan["argv"])
        self.assertFalse(life.thread.daemon)

    def test_start_failure_is_not_completed_process(self):
        def fail(*args, **kwargs):
            raise OSError("supplied-start-error")
        life = self.lifecycle(factory=fail)
        with self.assertRaisesRegex(FixtureError, "start failed"):
            life.start()
        with self.assertRaisesRegex(FixtureError, "termination unconfirmed"):
            life.finish(cancel=True)
        self.assertFalse(life.exit_observed)
        self.assertTrue(life.cancelled.is_set())

    def test_late_start_is_cancelled_once(self):
        def late(*args, **kwargs):
            self.clock.advance(9)
            return self.process
        life = self.lifecycle(factory=late)
        with self.assertRaisesRegex(FixtureError, "deadline"):
            life.start()
        life.finish(cancel=True)
        self.assertEqual(self.process.kills, 1)
        self.assertTrue(life.exit_observed and life.handles_closed)

    def test_start_after_hard_deadline_still_cancels(self):
        def late(*args, **kwargs):
            self.clock.advance(11)
            return self.process
        life = self.lifecycle(factory=late)
        with self.assertRaisesRegex(FixtureError, "deadline"):
            life.start()
        with self.assertRaisesRegex(FixtureError, "cleanup deadline"):
            life.finish(cancel=True)
        self.assertEqual(self.process.kills, 1)
        self.assertTrue(life.handles_closed)

    def test_cancel_before_creation_launches_nothing(self):
        life = self.lifecycle()
        life.cancel()
        life.launch_and_wait()
        self.assertEqual(self.calls, [])
        self.assertFalse(life.exit_observed)
        self.assertTrue(life.finished.is_set())

    def test_cancellation_after_budget_has_no_wait_or_false_success(self):
        life = self.lifecycle(thread=DeferredThread)
        life.process = self.process
        life.thread.alive = True
        self.clock.advance(11)
        with self.assertRaisesRegex(FixtureError, "termination unconfirmed"):
            life.finish(cancel=True)
        self.assertEqual(self.process.kills, 1)
        self.assertEqual(life.finished.waits, [])
        self.assertFalse(life.exit_observed)

    def test_multiple_cancel_requests_signal_owned_process_once(self):
        life = self.lifecycle()
        life.process = self.process
        life.cancel()
        life.cancel()
        self.assertEqual(self.process.kills, 1)

    def test_pending_creation_cancellation_remains_unconfirmed(self):
        life = self.lifecycle(thread=DeferredThread)
        with self.assertRaisesRegex(FixtureError, "startup unconfirmed"):
            life.start()
        with self.assertRaisesRegex(FixtureError, "termination unconfirmed"):
            life.finish(cancel=True)
        self.assertTrue(life.cancelled.is_set())
        self.assertEqual(self.calls, [])

    def test_capture_during_creation_after_cancel_is_killed(self):
        life = self.lifecycle()
        def create(*args, **kwargs):
            life.cancel()
            return self.process
        life.factory = create
        with self.assertRaisesRegex(FixtureError, "was cancelled"):
            life.start()
        life.finish(cancel=True)
        self.assertEqual(self.process.kills, 1)
        self.assertTrue(life.exit_observed)

    def test_wait_error_cannot_be_exit_receipt(self):
        self.process.wait_error = OSError("supplied-wait-error")
        life = self.lifecycle()
        with self.assertRaisesRegex(FixtureError, "start failed"):
            life.start()
        with self.assertRaisesRegex(FixtureError, "termination unconfirmed"):
            life.finish(cancel=True)
        self.assertEqual(self.process.kills, 1)
        self.assertFalse(life.handles_closed)

    def test_handle_close_failure_refuses_qualification(self):
        def fail_close(process):
            raise OSError("supplied-close-error")
        life = self.lifecycle(close=fail_close)
        life.start()
        life.wait_exit()
        with self.assertRaisesRegex(RuntimeError, "handle closure unconfirmed"):
            life.finish(cancel=True)
        self.assertTrue(life.exit_observed)
        self.assertFalse(life.handles_closed)

    def test_kill_failure_without_exit_remains_unconfirmed(self):
        self.process.kill_error = OSError("supplied-kill-error")
        life = self.lifecycle(thread=DeferredThread)
        life.process = self.process
        with self.assertRaisesRegex(FixtureError, "termination unconfirmed"):
            life.finish(cancel=True)
        self.assertEqual(life.snapshot()["kill_error_type"], "OSError")
        self.assertFalse(life.exit_observed)

    def test_nonzero_helper_exit_is_not_archive_success(self):
        self.process.code = 1
        life = self.lifecycle()
        life.start()
        with self.assertRaisesRegex(FixtureError, "helper failed"):
            life.wait_exit()
        life.finish(cancel=True)
        self.assertTrue(life.exit_observed and life.handles_closed)

    def test_finished_flag_without_actual_exit_refuses(self):
        life = self.lifecycle()
        life.finished.set()
        with self.assertRaisesRegex(FixtureError, "termination unconfirmed"):
            life.finish(cancel=False)

    def test_exit_claim_without_captured_process_refuses(self):
        life = self.lifecycle()
        life.finished.set()
        life.exit_observed = True
        with self.assertRaisesRegex(FixtureError, "termination unconfirmed"):
            life.finish(cancel=False)

    def test_thread_exit_still_required_after_process_exit(self):
        life = self.lifecycle()
        life.finished.set()
        life.exit_observed, life.handles_closed = True, True
        life.process = self.process
        life.thread.alive = True
        with self.assertRaisesRegex(FixtureError, "waiter termination"):
            life.finish(cancel=False)

    def test_late_process_exit_never_qualifies(self):
        self.process.wait_advance = 9
        life = self.lifecycle()
        with self.assertRaisesRegex(FixtureError, "deadline"):
            life.start()
        life.finish(cancel=True)
        self.assertTrue(life.exit_observed)
        self.assertLess(self.hard.clock(), self.hard.end)

    def test_windows_handle_is_explicitly_closed(self):
        handle = type("OwnedHandle", (), {"closed": False, "Close": lambda item: setattr(item, "closed", True)})()
        process = type("OwnedProcess", (), {"_handle": handle})()
        parent.close_process_handles(process, platform="win32")
        self.assertTrue(handle.closed)

    def test_missing_windows_handle_closure_is_refused(self):
        with self.assertRaisesRegex(FixtureError, "handle closure unsupported"):
            parent.close_process_handles(object(), platform="win32")

    def test_restricted_environment_drops_all_unlisted_values(self):
        environment = parent.helper_environment("/supplied/owned/home", {"HTTP_PROXY": "opaque-secret",
                    "PYTHONPATH": "opaque-hook", "PYTHONHOME": "opaque-home", "LD_PRELOAD": "opaque-library",
                    "CODETERM_API_TOKEN": "opaque-token", "OPENAI_API_KEY": "opaque-key"}, platform="linux")
        self.assertEqual(set(environment), {"HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "TMPDIR", "TMP", "TEMP", "PATH"})
        self.assertEqual(environment["PATH"], "")
        self.assertNotIn("opaque-secret", environment.values())

    def test_waiter_start_failure_requests_cancellation(self):
        class StartFailure(ImmediateThread):
            def start(self):
                raise OSError("supplied-thread-start-error")
        life = self.lifecycle(thread=StartFailure)
        with self.assertRaisesRegex(OSError, "supplied-thread-start-error"):
            life.start()
        self.assertTrue(life.cancelled.is_set())
        self.assertEqual(self.calls, [])
        with self.assertRaisesRegex(FixtureError, "termination unconfirmed"):
            life.finish(cancel=True)


class FakeResponse:
    def __init__(self, clock, reads, url="https://registry.npmjs.org/opaque.tgz", headers=(), status=200):
        self.clock, self.reads, self.url, self.status = clock, list(reads), url, status
        self.headers = Message()
        for key, value in headers:
            self.headers[key] = value
        self.limits, self.closed = [], False

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def close(self):
        self.closed = True

    def read(self, limit):
        self.limits.append(limit)
        value, advance = self.reads.pop(0)
        self.clock.advance(advance)
        return value


class FakeOpener:
    def __init__(self, response):
        self.response, self.timeouts = response, []

    def open(self, url, timeout):
        self.timeouts.append(timeout)
        return self.response


class HelperControls(unittest.TestCase):
    def setUp(self):
        tls_patch = patch.object(parent, "validated_tls", return_value=None)
        tls_patch.start()
        self.addCleanup(tls_patch.stop)
        self.clock = Clock()
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.archive = self.root / "supplied.tgz"
        self.body = b"supplied opaque archive bytes"
        self.metadata = {"tarball": "https://registry.npmjs.org/opaque.tgz",
                         "integrity": "sha512-" + base64.b64encode(hashlib.sha512(self.body).digest()).decode(),
                         "tarball_sha256": hashlib.sha256(self.body).hexdigest()}
        self.request = {"operation_deadline": 8, "metadata": self.metadata,
                        "fixture_root": str(self.root), "archive": str(self.archive)}

    def download(self, reads=None, **kwargs):
        response = FakeResponse(self.clock, reads or [(self.body, 0), (b"", 0)], **kwargs)
        return worker.download(self.request, FakeOpener(response), clock=self.clock), response

    def test_completed_helper_bytes_control(self):
        result, response = self.download(headers=[("Content-Length", str(len(self.body)))])
        self.assertEqual(result["sha256"], self.metadata["tarball_sha256"])
        self.assertEqual(self.archive.read_bytes(), self.body)
        self.assertTrue(response.closed)
        self.assertEqual(response.limits, [worker.CHUNK, worker.CHUNK])

    def test_partial_read_deadline_retains_incomplete_archive(self):
        response = FakeResponse(self.clock, [(b"a", 3), (b"b", 3), (b"c", 3)])
        with self.assertRaisesRegex(worker.AcquisitionError, "deadline"):
            worker.download(self.request, FakeOpener(response), clock=self.clock)
        self.assertEqual(self.archive.read_bytes(), b"ab")
        self.assertTrue(response.closed)

    def test_final_foreign_url_refuses_before_archive_creation(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "HTTPS confinement"):
            self.download(url="https://example.invalid/archive")
        self.assertFalse(self.archive.exists())

    def test_wrong_integrity_retains_rejected_bytes(self):
        self.request["metadata"] = self.metadata | {"integrity": "sha512-" + base64.b64encode(b"x" * 64).decode()}
        with self.assertRaisesRegex(worker.AcquisitionError, "SHA512 integrity differs"):
            self.download()
        self.assertEqual(self.archive.read_bytes(), self.body)

    def test_wrong_reviewed_sha256_refuses(self):
        self.request["metadata"] = self.metadata | {"tarball_sha256": "0" * 64}
        with self.assertRaisesRegex(worker.AcquisitionError, "SHA256 differs"):
            self.download()

    def test_archive_existing_destination_is_not_reused(self):
        self.archive.write_bytes(b"supplied previous evidence")
        with self.assertRaisesRegex(worker.AcquisitionError, "already exists"):
            self.download()
        self.assertEqual(self.archive.read_bytes(), b"supplied previous evidence")

    def test_escaped_archive_path_is_refused(self):
        self.request["archive"] = str(self.root.parent / "foreign.tgz")
        with self.assertRaisesRegex(worker.AcquisitionError, "escaped fixture"):
            self.download()

    def test_duplicate_length_headers_are_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "Ambiguous archive length"):
            self.download(headers=[("Content-Length", "1"), ("Content-Length", "2")])

    def test_declared_length_truncation_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "declared completion"):
            self.download(headers=[("Content-Length", str(len(self.body) + 1))])

    def test_encoded_body_is_not_raw_archive(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "Encoded archive"):
            self.download(headers=[("Content-Encoding", "gzip")])

    def test_header_count_is_bounded(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "headers exceed bound"):
            self.download(headers=[(f"supplied-{index}", "x") for index in range(worker.MAX_HEADERS + 1)])

    def test_oversized_header_line_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "headers exceed bound|header line exceeds"):
            self.download(headers=[("supplied", "x" * worker.MAX_HEADER_LINE)])

    def test_redirect_outside_registry_is_refused(self):
        redirect = worker.RegistryRedirect(8, self.clock)
        with self.assertRaisesRegex(worker.AcquisitionError, "HTTPS confinement"):
            redirect.redirect_request(None, None, 302, "opaque", Message(), "http://registry.npmjs.org/opaque")

    def test_redirect_hop_limit_is_refused(self):
        redirect = worker.RegistryRedirect(8, self.clock)
        redirect.hops = ["https://registry.npmjs.org/supplied"] * worker.MAX_REDIRECTS
        with self.assertRaisesRegex(worker.AcquisitionError, "redirect limit"):
            redirect.redirect_request(None, None, 302, "opaque", Message(), self.metadata["tarball"])

    def test_oversized_chunk_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "bounded chunk"):
            self.download(reads=[(b"x" * (worker.CHUNK + 1), 0)])

    def test_receipt_file_bound_refuses_unbounded_output(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "receipt exceeds bound"):
            worker.exclusive_json(self.root / "receipt.json", {"supplied": "x" * worker.MAX_RECEIPT})
        self.assertFalse((self.root / "receipt.json").exists())

    def test_partial_json_is_not_receipt(self):
        path = self.root / "receipt.json"
        path.write_bytes(b'{"supplied":')
        with self.assertRaises(json.JSONDecodeError):
            worker.bounded_json(path, worker.MAX_RECEIPT)

    def test_credential_url_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "HTTPS confinement"):
            worker.registry_url("https://supplied-secret@registry.npmjs.org/opaque")

    def test_invalid_pinned_digest_is_refused(self):
        with self.assertRaisesRegex(worker.AcquisitionError, "SHA512 length differs"):
            worker.metadata_input(self.metadata | {"integrity": "sha512-eA=="})

    def test_parent_receipt_prose_without_envelope_is_refused(self):
        workspace = self.root / "supplied.tgz.acquisition"
        workspace.mkdir()
        request = {"fixture_root": str(self.root)}
        path = workspace / "request.json"
        worker.exclusive_json(path, request)
        raw = path.read_bytes()
        receipt = workspace / "receipt.json"
        worker.exclusive_json(receipt, {"status": "completed"})
        plan = {"request": request, "request_path": path, "receipt_path": receipt,
                "argv": [hashlib.sha256(raw).hexdigest()], "destination": self.archive}
        with self.assertRaisesRegex(FixtureError, "receipt envelope differs"):
            parent.validated_result(plan, Deadline(10, self.clock))

    def test_changed_request_is_refused_before_receipt(self):
        path = self.root / "request.json"
        path.write_text('{}')
        plan = {"request": {"fixture_root": str(self.root)}, "request_path": path,
                "receipt_path": self.root / "receipt.json", "argv": ["supplied-original-hash"]}
        with self.assertRaisesRegex(FixtureError, "request changed"):
            parent.validated_result(plan, Deadline(10, self.clock))

    def receipt_plan(self):
        workspace = self.root / "supplied.tgz.acquisition"
        workspace.mkdir()
        self.archive.write_bytes(self.body)
        request = self.request | {"receipt": str(workspace / "receipt.json"),
                   "worker_sha256": "supplied-input-digest", "interpreter": "supplied-interpreter",
                   "interpreter_sha256": "supplied-input-digest"}
        path = workspace / "request.json"
        worker.exclusive_json(path, request)
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        plan = {"request": request, "request_path": path, "receipt_path": workspace / "receipt.json",
                "argv": [digest], "destination": self.archive}
        receipt = {"version": 1, "status": "completed", "request_sha256": digest, "archive": str(self.archive),
                   "result": {"sha512": self.metadata["integrity"], "sha256": self.metadata["tarball_sha256"],
                              "bytes": len(self.body), "source": self.metadata["tarball"],
                              "final_url": self.metadata["tarball"], "redirects": []}}
        return plan, receipt

    def test_parent_independently_checks_actual_archive_bytes(self):
        plan, receipt = self.receipt_plan()
        worker.exclusive_json(plan["receipt_path"], receipt)
        with patch.object(parent, "file_digest", return_value="supplied-input-digest"):
            result = parent.validated_result(plan, Deadline(10, self.clock))
        self.assertEqual(result["sha256"], self.metadata["tarball_sha256"])
        self.assertNotIn("status", result)

    def test_changed_bytes_cannot_use_success_prose(self):
        plan, receipt = self.receipt_plan()
        worker.exclusive_json(plan["receipt_path"], receipt)
        self.archive.write_bytes(b"supplied changed bytes")
        with self.assertRaisesRegex(FixtureError, "SHA512 integrity differs"):
            parent.validated_result(plan, Deadline(10, self.clock))

    def test_receipt_byte_count_must_equal_actual_file(self):
        plan, receipt = self.receipt_plan()
        receipt["result"]["bytes"] += 1
        worker.exclusive_json(plan["receipt_path"], receipt)
        with self.assertRaisesRegex(FixtureError, "bytes differ from receipt"):
            parent.validated_result(plan, Deadline(10, self.clock))

    def test_receipt_final_url_must_join_actual_redirect_list(self):
        plan, receipt = self.receipt_plan()
        receipt["result"]["final_url"] = "https://registry.npmjs.org/foreign"
        worker.exclusive_json(plan["receipt_path"], receipt)
        with self.assertRaisesRegex(FixtureError, "redirect join differs"):
            parent.validated_result(plan, Deadline(10, self.clock))

    def test_boolean_version_is_not_exact_envelope_version(self):
        plan, receipt = self.receipt_plan()
        receipt["version"] = True
        worker.exclusive_json(plan["receipt_path"], receipt)
        with self.assertRaisesRegex(FixtureError, "receipt envelope differs"):
            parent.validated_result(plan, Deadline(10, self.clock))

    def test_unconfirmed_owned_exit_preserves_parent_failure_and_blocks_validation(self):
        plan, receipt = self.receipt_plan()
        self.archive.write_bytes(b"supplied partial bytes")
        finishes = []
        class Unconfirmed:
            def start(self):
                pass
            def wait_exit(self):
                raise FixtureError("Acquisition process exit unconfirmed")
            def finish(self, cancel):
                finishes.append(cancel)
                raise FixtureError("Acquisition owned process termination unconfirmed")
            def snapshot(self):
                return {"exit_observed": False, "handles_closed": False}
        with patch.object(parent, "validated_result") as validation:
            with self.assertRaisesRegex(FixtureError, "termination unconfirmed"):
                parent.acquire_owned(self.metadata, self.archive, 10, self.clock,
                                     lambda *args: plan, lambda *args: Unconfirmed())
            validation.assert_not_called()
        self.assertEqual(finishes, [True])
        self.assertEqual(self.archive.read_bytes(), b"supplied partial bytes")
        _, status = worker.bounded_json(plan["receipt_path"].with_name("parent.json"), worker.MAX_RECEIPT)
        self.assertFalse(status["lifecycle"]["exit_observed"])

    def test_redirect_closes_prior_response_without_draining_body(self):
        response = FakeResponse(self.clock, [])
        headers = Message()
        headers["Location"] = "/supplied-target"
        redirect = worker.RegistryRedirect(8, self.clock)
        opens = []
        class Open:
            def open(self, request, timeout):
                opens.append((request.full_url, timeout))
                return "supplied-response"
        redirect.parent = Open()
        value = redirect.http_error_302(worker.urllib.request.Request(self.metadata["tarball"]), response,
                                         302, "opaque", headers)
        self.assertEqual(value, "supplied-response")
        self.assertTrue(response.closed)
        self.assertEqual(response.limits, [])
        self.assertEqual(opens, [("https://registry.npmjs.org/supplied-target", 8)])

    def test_ambiguous_redirect_closes_response_without_opening_target(self):
        response = FakeResponse(self.clock, [])
        headers = Message()
        headers["Location"] = "/one"
        headers["Location"] = "/two"
        with self.assertRaisesRegex(worker.AcquisitionError, "location ambiguous"):
            worker.RegistryRedirect(8, self.clock).http_error_302(
                worker.urllib.request.Request(self.metadata["tarball"]), response, 302, "opaque", headers)
        self.assertTrue(response.closed)
        self.assertEqual(response.limits, [])
