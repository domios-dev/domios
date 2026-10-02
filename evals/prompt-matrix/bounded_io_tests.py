"""Deterministic IO/deadline and tar controls; no real socket, thread or network."""

import base64
import errno
import hashlib
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import busy_fixture
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

    def test_default_transport_refuses_before_any_network(self):
        with self.assertRaisesRegex(FixtureError, "transport unavailable"):
            acquire(self.metadata, self.path, 10, clock=self.clock)
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
