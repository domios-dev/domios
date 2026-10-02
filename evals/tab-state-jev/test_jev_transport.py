"""WHAT: byte/deadline bounds, event ordering, authority and completion admission.
WHY: EOF, status JSON or a parent return cannot prove exact helper completion.
ORACLE: injected reads/events/clock/authority expose each missing completion bit.
Synthetic mechanism fixtures only; no sockets, native children or clock sleeps.
"""

from contextlib import ExitStack, redirect_stdout
import io
import json
import os
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import local_jev_transport_io as local
import jev_request_worker as w
import jev_transport as t
import request_contract as c


class Clock:
    def __init__(self, value=0):
        self.value = value

    def __call__(self):
        return self.value


class SocketSource:
    def __init__(self, raw, *, short=4096, callback=None):
        self.raw, self.short, self.callback = raw, short, callback
        self.bounds, self.timeouts = [], []

    def settimeout(self, timeout):
        self.timeouts.append(timeout)

    def recv(self, size):
        self.bounds.append(size)
        if self.callback:
            self.callback()
        chunk, self.raw = self.raw[:min(size, self.short)], self.raw[min(size, self.short):]
        return chunk


class InlineThread:
    def __init__(self, target, name, args=(), daemon=False):
        self.target, self.args, self.daemon = target, args, daemon

    def start(self):
        self.target(*self.args)

    def join(self, timeout=None):
        return None

    def is_alive(self):
        return False


class AuthorityFixture:
    def __init__(self, process):
        self.process, self.reaped, self.closed, self.signals = process, False, False, []

    def wait_exact(self):
        self.reaped = True
        return 0

    def cancel(self):
        if not self.reaped:
            self.signals.append("cancel")
            return True
        return False

    def close(self):
        self.closed = True


def synthetic_wire():
    return c.encode({"state": "synthetic mechanism", "model": "fixture-alias", "questions": {
        "q": {"type": "choice", "instructions": "opaque", "criteria": {"x": "opaque"}}}})


def lease_fixture(clock=None, *, spawn_callback=None, stdout=b"{}"):
    clock = clock or Clock()
    process = SimpleNamespace(pid=123, returncode=None, stdin=io.BytesIO(), stdout=io.BytesIO(stdout), stderr=io.BytesIO())
    def spawn(plan):
        if spawn_callback:
            spawn_callback()
        return process
    lease = t.OwnedLease({"cwd": Path("/synthetic-mechanism"), "header": {}}, 55, 60,
                         clock=clock, spawn=spawn, authority_factory=AuthorityFixture, thread_factory=InlineThread)
    return lease, process


class ByteBoundTests(unittest.TestCase):
    def test_small_positive_and_short_reads_use_positive_chunk_bounds(self):
        for short in (1, 17, 4096):
            source = SocketSource(b"opaque" * 100, short=short)
            reader = w.SocketReader(source, 55, lambda: 0)
            self.assertEqual(w.read_bounded_response(reader, end=55, clock=lambda: 0), b"opaque" * 100)
            self.assertTrue(all(0 < size <= 4096 for size in source.bounds))

    def test_response_oversize_rejected_before_parsing(self):
        source = io.BytesIO(b"x" * (65536 + 1))
        with self.assertRaisesRegex(w.WorkerError, "response_oversize") as caught:
            w.read_bounded_response(source, end=55, clock=lambda: 0)
        self.assertEqual(caught.exception.facts["response_bytes"], 65537)

    def test_chunked_positive_and_oversize_are_bounded(self):
        for body, failed in ((b"opaque", False), (b"x" * 65537, True)):
            raw = hex(len(body))[2:].encode() + b"\r\n" + body + b"\r\n0\r\n\r\n"
            response = w.HttpBody(w.SocketReader(SocketSource(raw, short=3), 55, lambda: 0), 200,
                                  {"transfer-encoding": "chunked"})
            if failed:
                with self.assertRaisesRegex(w.WorkerError, "response_oversize"):
                    w.read_bounded_response(response, end=55, clock=lambda: 0)
            else:
                self.assertEqual(w.read_bounded_response(response, end=55, clock=lambda: 0), body)

    def test_slow_headers_use_original_deadline(self):
        clock = Clock()
        def advance():
            clock.value += 20
        reader = w.SocketReader(SocketSource(b"Header: opaque\r\n", short=1, callback=advance), 55, clock)
        with self.assertRaisesRegex(w.WorkerError, "deadline"):
            reader.line()
        self.assertEqual(reader.end, 55)

    def test_headers_bounded_before_decode(self):
        reader = w.SocketReader(SocketSource(b"X:" + b"x" * 16384 + b"\r\n"), 55, lambda: 0)
        with self.assertRaisesRegex(w.WorkerError, "header_oversize"):
            reader.headers()

    def test_header_field_count_and_duplicates_rejected(self):
        for raw, code in ((b"".join(f"X{i}: x\r\n".encode() for i in range(101)) + b"\r\n", "header_fields"),
                          (b"X: one\r\nx: two\r\n\r\n", "duplicate_header")):
            with self.assertRaisesRegex(w.WorkerError, code):
                w.SocketReader(SocketSource(raw), 55, lambda: 0).headers()

    def test_error_diagnostics_refuse_excess_instead_of_truncating(self):
        self.assertEqual(w.read_bounded_response(io.BytesIO(b"x" * 4096), limit=4096,
                                                end=55, clock=lambda: 0), b"x" * 4096)
        with self.assertRaisesRegex(w.WorkerError, "error_body_oversize"):
            w.read_bounded_response(io.BytesIO(b"x" * 4097), limit=4096, end=55, clock=lambda: 0)

    def test_content_encoding_ambiguous_length_and_truncation_rejected(self):
        reader = w.SocketReader(SocketSource(b"x"), 55, lambda: 0)
        for headers in ({"content-encoding": "gzip"}, {"content-length": "1", "transfer-encoding": "chunked"},
                        {"content-length": "65537"}):
            with self.assertRaises(w.WorkerError):
                w.HttpBody(reader, 200, headers)
        response = w.HttpBody(reader, 200, {"content-length": "2"})
        with self.assertRaisesRegex(w.WorkerError, "body_incomplete"):
            w.read_bounded_response(response, end=55, clock=lambda: 0)

    def test_redirect_refused_through_actual_exchange(self):
        stream = Mock()
        source = SocketSource(b"HTTP/1.1 302 Found\r\nLocation: https://elsewhere.invalid\r\n\r\n")
        stream.recv.side_effect = source.recv
        stream.send.side_effect = lambda data: len(data)
        context = Mock(verify_mode=w.ssl.CERT_REQUIRED, check_hostname=True, keylog_filename=None)
        context.get_ca_certs.return_value = [b"synthetic public-root bytes"]
        context.wrap_socket.return_value = stream
        with patch.object(w, "trust_context", return_value=(context, "synthetic mechanism")), \
                patch.object(w.socket, "create_connection", return_value=Mock()) as connect:
            with self.assertRaisesRegex(w.WorkerError, "redirect_refused"):
                w.https_exchange(synthetic_wire(), b"synthetic-token", 55, clock=lambda: 0)
        self.assertEqual(connect.call_count, 1)
        stream.close.assert_called_once()

    def test_tls_verification_cannot_be_weakened(self):
        for mode, hostname in ((w.ssl.CERT_NONE, True), (w.ssl.CERT_REQUIRED, False)):
            context = Mock(verify_mode=mode, check_hostname=hostname, keylog_filename=None)
            with self.assertRaisesRegex(w.WorkerError, "tls_verification"):
                w.trust_receipt(context, "synthetic mechanism")


class AdmissionTests(unittest.TestCase):
    def test_absent_token_has_zero_creation_and_network(self):
        plan = Mock()
        transport = t.JevTransport(clock=Clock(), plan_factory=plan)
        with self.assertRaisesRegex(t.TransportFailure, "token_preflight"):
            transport.send(synthetic_wire(), token="")
        plan.assert_not_called()
        self.assertEqual(transport.calls, 0)

    def test_ambient_proxy_and_provider_secrets_not_in_child_environment(self):
        with patch.dict(os.environ, {"https_proxy": "https://synthetic-proxy.invalid", "PYTHONPATH": "/synthetic",
                                    "CODETERM_API_TOKEN": "synthetic-daemon", "JEV_API_TOKEN": "synthetic-token"}):
            environment = t.minimal_environment(platform="posix")
        self.assertFalse(any(key.lower().endswith("proxy") or key.startswith(("PYTHON", "CODETERM", "JEV"))
                             for key in environment))
        self.assertNotIn("synthetic-token", json.dumps(environment))

    def test_nonfixed_endpoint_refused_before_creation(self):
        plan = Mock()
        transport = t.JevTransport(clock=Clock(), plan_factory=plan)
        for endpoint in ("http://api.typesafe.ai/v1/systemone", c.ENDPOINT + "/", "https://elsewhere.invalid"):
            with self.assertRaisesRegex(t.TransportFailure, "endpoint_policy"):
                transport.send(synthetic_wire(), token="synthetic-token", endpoint=endpoint)
        plan.assert_not_called()

    def test_original_deadlines_and_cleanup_reserve_never_reset(self):
        self.assertEqual(t.request_deadlines(10, 600, 60), (65, 70))
        self.assertEqual(t.request_deadlines(570, 600, 60), (595, 600))
        for timeout in (5, 61, float("inf"), float("nan"), True):
            with self.assertRaises(t.TransportFailure):
                t.request_deadlines(0, 600, timeout)

    def test_count_ceiling_has_no_extra_call(self):
        plan = Mock()
        transport = t.JevTransport(clock=Clock(), plan_factory=plan)
        transport.calls = 300
        with self.assertRaisesRegex(t.TransportFailure, "request_count"):
            transport.send(synthetic_wire(), token="synthetic-token")
        plan.assert_not_called()

    def test_windows_admission_requires_exact_native_runtime(self):
        with patch.object(t.os, "name", "nt"), patch.object(t.sys, "platform", "win32"), \
                patch.object(t.sys, "version_info", (3, 14, 5)), \
                patch.object(w.sysconfig, "get_platform", return_value="win-amd64"):
            with self.assertRaisesRegex(t.TransportFailure, "unsupported_windows_runtime"):
                t.native_support()


class OwnershipTests(unittest.TestCase):
    def test_exact_event_then_reap_and_no_post_reap_signal(self):
        log = []
        process = SimpleNamespace(pid=123, returncode=None)
        authority = t.ProcessAuthority(process, observer=lambda pid: log.append(("event", pid)),
                                       reaper=lambda pid, options: (log.append(("reap", pid)) or (pid, 0)),
                                       signaler=lambda *args: log.append(("signal", args)), windows=False,
                                       status_converter=lambda status: status, kill_signal=9)
        self.assertEqual(authority.wait_exact(), 0)
        self.assertFalse(authority.cancel())
        authority.close()
        self.assertEqual(log, [("event", 123), ("reap", 123)])
        self.assertTrue(authority.closed)

    def test_cancel_before_reap_keeps_original_authority(self):
        signaler = Mock()
        authority = t.ProcessAuthority(SimpleNamespace(pid=123, returncode=None), observer=lambda pid: None,
                                       reaper=lambda pid, options: (pid, 0), signaler=signaler, windows=False,
                                       status_converter=lambda status: status, kill_signal=9)
        self.assertTrue(authority.cancel())
        self.assertFalse(authority.cancel())
        authority.wait_exact()
        self.assertFalse(authority.cancel())
        signaler.assert_called_once_with(123, 9)

    def test_lost_reap_authority_never_signals_stale_pid(self):
        signaler = Mock()
        authority = t.ProcessAuthority(SimpleNamespace(pid=123, returncode=None),
                                       observer=Mock(side_effect=ChildProcessError()), signaler=signaler, windows=False)
        with self.assertRaises(ChildProcessError):
            authority.wait_exact()
        self.assertFalse(authority.cancel())
        signaler.assert_not_called()

    def test_windows_uses_captured_handle_not_numeric_pid(self):
        class HandleFixture:
            def __int__(self):
                return 77
        handle = HandleFixture()
        handle.Close = Mock()
        kernel = Mock()
        kernel.WaitForSingleObject.return_value = 0
        kernel.TerminateProcess.return_value = 1
        def code(handle_value, pointer):
            pointer._obj.value = 0
            return 1
        kernel.GetExitCodeProcess.side_effect = code
        with patch.object(t.ctypes, "WinDLL", return_value=kernel, create=True):
            authority = t.ProcessAuthority(SimpleNamespace(pid=999, returncode=None, _handle=handle), windows=True)
            authority.cancel()
            authority.wait_exact()
            self.assertFalse(authority.cancel())
            authority.close()
        kernel.TerminateProcess.assert_called_once_with(77, 1)
        kernel.WaitForSingleObject.assert_called_once_with(77, 0xFFFFFFFF)
        handle.Close.assert_called_once()

    def test_eof_before_exit_cannot_settle(self):
        lease, process = lease_fixture()
        lease._capture("stdout", process.stdout)
        self.assertTrue(lease.facts["stdout_complete"])
        self.assertFalse(t.settlement(lease.facts))

    def test_exit_before_capture_cannot_settle(self):
        lease, process = lease_fixture()
        lease.authority = AuthorityFixture(process)
        lease._exit()
        self.assertTrue(lease.facts["exit_observed"])
        self.assertFalse(t.settlement(lease.facts))

    def test_every_exact_completion_boundary_is_required(self):
        facts = {key: True for key in ("start_finished", "created", "exit_observed", "reap_confirmed",
                 "stdout_complete", "stderr_complete", "stdout_closed", "stderr_closed",
                 "credential_channel_closed", "threads_joined", "handle_closed", "cwd_closed")}
        self.assertTrue(t.settlement(facts))
        for key in facts:
            self.assertFalse(t.settlement({**facts, key: False}), key)
        self.assertFalse(t.settlement({**facts, "ownership_lost": True}))

    def test_cancel_before_start_admits_no_credentials(self):
        lease, _ = lease_fixture()
        lease.spawn = Mock()
        lease.cancel()
        lease.start()
        lease.spawn.assert_not_called()
        with self.assertRaises(t.TransportFailure):
            lease.admit(synthetic_wire(), b"synthetic-token")

    def test_creation_returning_late_is_cancelled_before_credential_admission(self):
        clock = Clock()
        lease, process = lease_fixture(clock, spawn_callback=lambda: setattr(clock, "value", 56))
        lease.start()
        self.assertTrue(lease.facts["created"])
        self.assertTrue(lease.cancelled)
        self.assertTrue(process.stdin.closed)
        self.assertFalse(lease.input_started)
        with self.assertRaises(t.TransportFailure):
            lease.admit(synthetic_wire(), b"synthetic-token")

    def test_cancel_during_creation_precedes_credential_handoff(self):
        lease, _ = lease_fixture()
        spawn = lease.spawn
        def cancelled_spawn(plan):
            lease.cancel()
            return spawn(plan)
        lease.spawn = cancelled_spawn
        lease.start()
        self.assertTrue(lease.cancelled)
        self.assertFalse(lease.input_started)
        self.assertTrue(lease.facts["credential_channel_closed"])

    def test_capture_failure_retains_partial_size_and_closes_owned_stream(self):
        lease, process = lease_fixture()
        source = Mock()
        source.read.side_effect = [b"partial", OSError("synthetic failure")]
        lease.capture_source = lambda name, stream: source
        lease._capture("stdout", process.stdout)
        self.assertEqual(lease.facts["stdout_bytes"], 7)
        self.assertFalse(lease.facts["stdout_complete"])
        self.assertTrue(lease.facts["stdout_closed"])
        self.assertFalse(t.settlement(lease.facts))

    def test_capture_oversize_cannot_become_valid_truncated_json(self):
        lease, process = lease_fixture(stdout=b"{}" + b" " * 4096)
        lease._capture("stdout", process.stdout)
        self.assertEqual(lease.facts["error"], "capture_oversize")
        self.assertFalse(lease.facts["stdout_complete"])
        self.assertEqual(lease.stdout, b"")

    def test_handle_close_failure_prevents_confirmed_completion(self):
        lease, _ = lease_fixture()
        lease.start()
        lease._close_input()
        lease.authority.close = Mock(side_effect=OSError("synthetic close error"))
        with patch.object(t.shutil, "rmtree"):
            receipt = lease.finish()
        self.assertEqual(receipt["error"], "cleanup_unconfirmed")
        self.assertFalse(receipt["handle_closed"])
        self.assertFalse(receipt["completion_confirmed"])

    def test_event_wait_uses_original_absolute_failure_end(self):
        clock = Clock()
        lease, _ = lease_fixture(clock)
        class ConditionSource:
            def __enter__(self):
                return self
            def __exit__(self, *args):
                return False
            def wait(self, timeout):
                self.bound = timeout
                clock.value = 55
        condition = ConditionSource()
        lease.condition = condition
        with self.assertRaisesRegex(t.TransportFailure, "deadline"):
            lease.await_state(lambda facts: False, 55)
        self.assertEqual(condition.bound, 55)
        self.assertEqual(lease.operation_end, 55)

    def test_unconfirmed_completion_blocks_next_request(self):
        factory = Mock()
        transport = t.JevTransport(clock=Clock(), plan_factory=factory)
        transport.poisoned = True
        with self.assertRaisesRegex(t.TransportFailure, "lease_unavailable"):
            transport.send(synthetic_wire(), token="synthetic-token")
        factory.assert_not_called()

    def test_positive_owned_completion_waits_for_all_captures_and_closes(self):
        lease, process = lease_fixture()
        lease.start()
        lease._close_input()
        with patch.object(t.shutil, "rmtree") as remove:
            receipt = lease.finish()
        self.assertTrue(receipt["completion_confirmed"])
        self.assertIsNone(receipt["error"])
        self.assertTrue(process.stdout.closed and process.stderr.closed and process.stdin.closed)
        remove.assert_called_once_with(Path("/synthetic-mechanism"))

    def test_creation_os_error_never_has_completion_receipt(self):
        lease, _ = lease_fixture()
        lease.spawn = Mock(side_effect=OSError("synthetic-token must not be logged"))
        lease.start()
        self.assertEqual(lease.facts["error"], "creation_os")
        self.assertFalse(t.settlement(lease.facts))
        self.assertNotIn("synthetic-token", json.dumps(lease.facts))

    def test_cancel_after_response_before_settlement_prevents_success(self):
        lease, _ = lease_fixture(stdout=b'{"status":"completed"}')
        lease.start()
        lease._close_input()
        lease.cancel()
        with patch.object(t.shutil, "rmtree"):
            receipt = lease.finish()
        self.assertTrue(receipt["completion_confirmed"])
        self.assertEqual(receipt["error"], "cancelled")
        self.assertTrue(lease.cancelled)
        self.assertEqual(lease.authority.signals, [])

    def test_stderr_capture_is_bounded_and_never_exported(self):
        lease, process = lease_fixture()
        process.stderr = io.BytesIO(b"synthetic-token" + b"x" * 4096)
        lease._capture("stderr", process.stderr)
        self.assertEqual(lease.facts["error"], "capture_oversize")
        self.assertFalse(lease.facts["stderr_complete"])
        self.assertTrue(lease.facts["stderr_closed"])
        self.assertNotIn("synthetic-token", json.dumps(lease.facts))

    def test_channel_close_error_cannot_settle(self):
        lease, process = lease_fixture()
        process.stdin = Mock()
        process.stdin.close.side_effect = OSError("synthetic close failure")
        lease._close_input()
        self.assertEqual(lease.facts["error"], "channel_close")
        self.assertFalse(lease.facts["credential_channel_closed"])
        self.assertFalse(t.settlement(lease.facts))

    def test_credential_is_absent_from_prepared_argv_environment_and_files(self):
        synthetic_root = Path("/synthetic-mechanism")
        fixture_stat = SimpleNamespace(st_uid=5, st_mode=0o40700)
        with patch.object(t, "native_support"), patch.object(t, "identity_hash", side_effect=[
                (Path("/synthetic-helper.py"), "a" * 64),
                (Path("/synthetic-contract.py"), "b" * 64),
                (Path("/synthetic-interpreter"), "c" * 64)]), \
                patch.object(t.tempfile, "mkdtemp", return_value=str(synthetic_root)), \
                patch.object(t.Path, "resolve", lambda path, **kwargs: path), \
                patch.object(t.Path, "stat", return_value=fixture_stat), \
                patch.object(t.Path, "is_symlink", return_value=False), \
                patch.object(t.os, "getuid", return_value=5, create=True), \
                patch.object(t, "minimal_environment", return_value={"LANG": "C"}), \
                patch("builtins.open") as files:
            plan = t.prepare_plan(synthetic_wire(), 55, 60, Clock())
        self.assertNotIn("synthetic-token", json.dumps({key: str(value) for key, value in plan.items()}))
        self.assertEqual(plan["argv"][1:4], ["-I", "-S", "-B"])
        self.assertEqual(plan["env"], {"LANG": "C"})
        files.assert_not_called()

    def test_invalid_token_cannot_inject_authorization_headers(self):
        for token in ("", "synthetic\r\nInjected: header", " synthetic", "synthetic ", "x" * 4097):
            with self.assertRaisesRegex(t.TransportFailure, "token_preflight"):
                t.token_bytes(token)
        self.assertEqual(t.token_bytes("synthetic-token"), b"synthetic-token")


class HarnessRegressionTests(unittest.TestCase):
    def harness_patches(self, stack):
        stack.enter_context(patch.object(local.sys, "argv", ["local_jev_transport_io.py", "--case", "success"]))
        stack.enter_context(patch.object(local.time, "monotonic", Clock()))
        stack.enter_context(patch.object(t, "native_support"))
        temporary = stack.enter_context(patch.object(local.tempfile, "TemporaryDirectory"))
        temporary.return_value.__enter__.return_value = "/synthetic-mechanism"
        stack.enter_context(patch.object(local.Path, "write_bytes"))
        stack.enter_context(patch.object(local, "public_fixture_certificate", return_value=(b"public-ca", b"public-key")))
        stack.enter_context(patch.object(local.ssl, "SSLContext"))
        fixture_factory = stack.enter_context(patch.object(local, "LoopbackFixture"))
        fixture_receipt = {"requests": 0, "responses": 0, "accepted_closed": False,
                           "listener_closed": True, "thread_joined": True, "error": None}
        fixture_factory.return_value.close.return_value = fixture_receipt
        return fixture_factory, fixture_receipt

    def test_preflight_refusal_never_opens_listener(self):
        with ExitStack() as stack:
            fixture_factory, _ = self.harness_patches(stack)
            prepare = stack.enter_context(patch.object(t, "prepare_plan", side_effect=
                                                      t.TransportFailure("trusted_file_permissions")))
            output = io.StringIO()
            with redirect_stdout(output):
                self.assertEqual(local.main(), 1)
            prepare.assert_called_once()
            fixture_factory.assert_not_called()
        report = json.loads(output.getvalue())
        self.assertEqual(report["error"], "trusted_file_permissions")
        self.assertFalse(report["fixture"]["listener_opened"])

    def test_failed_receipt_keeps_primary_and_fixture_cleanup(self):
        transport_receipt = {"error": "trusted_file_permissions", "phase": "preparation",
                             "process_admitted": False, "completion_confirmed": False}
        with ExitStack() as stack:
            _, fixture_receipt = self.harness_patches(stack)
            stack.enter_context(patch.object(t, "prepare_plan", return_value={
                "argv": ["/synthetic-interpreter", "-I", "-S", "-B", "/synthetic-helper"],
                "cwd": Path("/synthetic-mechanism"), "header": {"interpreter": "/synthetic-interpreter"}}))
            stack.enter_context(patch.object(local, "fixture_plan_factory", return_value=Mock()))
            stack.enter_context(patch.object(t.shutil, "rmtree"))
            transport = stack.enter_context(patch.object(t, "JevTransport"))
            transport.return_value.send.side_effect = t.TransportFailure("trusted_file_permissions", transport_receipt)
            output = io.StringIO()
            with redirect_stdout(output):
                self.assertEqual(local.main(), 1)
        report = json.loads(output.getvalue())
        self.assertEqual(report["error"], "trusted_file_permissions")
        self.assertEqual(report.get("transport"), transport_receipt)
        self.assertEqual(report.get("fixture"), fixture_receipt)
        self.assertEqual(report.get("oracle_error"), "fixture_wrong_predicate")


class HarnessBoundaryTests(unittest.TestCase):
    def fixture_patches(self, stack, case="success"):
        stack.enter_context(patch.object(local.time, "monotonic", Clock()))
        listener, plain, stream, reader, writer, selector, context = (Mock() for _ in range(7))
        listener.getsockname.return_value = ("127.0.0.1", 43210)
        listener.accept.return_value = (plain, ("127.0.0.1", 12345))
        context.wrap_socket.return_value = stream
        writer.send.return_value = 1
        reader.recv.return_value = b"C"
        selector.select.return_value = [(SimpleNamespace(data="listener"), local.selectors.EVENT_READ)]
        stack.enter_context(patch.object(local.socket, "socket", return_value=listener))
        stack.enter_context(patch.object(local.socket, "socketpair", return_value=(reader, writer)))
        stack.enter_context(patch.object(local.selectors, "DefaultSelector", return_value=selector))
        stack.enter_context(patch.object(local.threading, "Thread", InlineThread))
        request = stack.enter_context(patch.object(local, "read_request", return_value=b"exact-synthetic-wire"))
        fixture = local.LoopbackFixture(case, context, 55, 60)
        return SimpleNamespace(fixture=fixture, listener=listener, plain=plain, stream=stream,
                               reader=reader, writer=writer, selector=selector, context=context, request=request)

    def assert_closed_fixture(self, receipt):
        self.assertTrue(all(receipt[key] for key in ("listener_closed", "selector_closed", "wake_reader_closed",
                                                    "wake_writer_closed", "thread_joined", "retirement_observed")))
        self.assertEqual(receipt["cleanup_errors"], [])
        self.assertIsNone(receipt["error"])

    def test_pre_accept_cancellation_wakes_owned_selector(self):
        with ExitStack() as stack:
            source = self.fixture_patches(stack)
            def cancelled_ready(timeout):
                self.assertEqual(timeout, 55)
                self.assertTrue(source.fixture.ready.is_set())
                source.fixture.request_cancel()
                return [(SimpleNamespace(data="cancel"), local.selectors.EVENT_READ)]
            source.selector.select.side_effect = cancelled_ready
            source.fixture.start()
            receipt = source.fixture.close()
        source.writer.send.assert_called_once_with(b"C")
        source.reader.recv.assert_called_once_with(1)
        source.listener.accept.assert_not_called()
        source.context.wrap_socket.assert_not_called()
        source.request.assert_not_called()
        self.assertTrue(receipt["wake_observed"] and receipt["cancel_observed"])
        self.assertEqual(receipt["cancellation_phase"], "pre_accept")
        self.assertFalse(receipt["accept_observed"])
        self.assertEqual((receipt["requests"], receipt["responses"]), (0, 0))
        self.assert_closed_fixture(receipt)

    def test_accepted_handshake_cancellation_uses_published_tls_handle(self):
        with ExitStack() as stack:
            source = self.fixture_patches(stack)
            def cancelled_handshake():
                self.assertIs(source.fixture.accepted, source.stream)
                source.fixture.request_cancel()
                raise OSError("synthetic interrupted handshake")
            source.stream.do_handshake.side_effect = cancelled_handshake
            source.fixture.start()
            receipt = source.fixture.close()
        source.context.wrap_socket.assert_called_once_with(source.plain, server_side=True,
                                                           do_handshake_on_connect=False)
        source.stream.shutdown.assert_called_once_with(local.socket.SHUT_RDWR)
        source.stream.close.assert_called_once()
        source.request.assert_not_called()
        self.assertTrue(receipt["accepted_closed"] and receipt["cancel_observed"])
        self.assertEqual(receipt["cancellation_phase"], "accepted")
        self.assert_closed_fixture(receipt)

    def test_accepted_headers_cancellation_releases_owned_hold(self):
        with ExitStack() as stack:
            source = self.fixture_patches(stack, "cancel_after_headers")
            source.stream.sendall.side_effect = lambda data: source.fixture.request_cancel()
            source.fixture.start()
            receipt = source.fixture.close()
        self.assertTrue(source.fixture.headers_sent.is_set() and source.fixture.release.is_set())
        self.assertEqual((receipt["requests"], receipt["responses"]), (1, 1))
        self.assertFalse(receipt["body_completed"])
        self.assertTrue(receipt["cancel_observed"] and receipt["accepted_closed"])
        source.stream.shutdown.assert_called_once_with(local.socket.SHUT_RDWR)
        self.assert_closed_fixture(receipt)

    def test_positive_fixture_preserves_one_wire_and_complete_response(self):
        with ExitStack() as stack:
            source = self.fixture_patches(stack)
            source.fixture.start()
            receipt = source.fixture.close()
        sent = b"".join(call.args[0] for call in source.stream.sendall.call_args_list)
        headers, body = sent.split(b"\r\n\r\n", 1)
        self.assertIn(b"Content-Length: " + str(len(body)).encode(), headers)
        self.assertEqual(source.fixture.request_bytes, b"exact-synthetic-wire")
        self.assertEqual(source.fixture.response_bytes_sha256, c.sha256(body))
        self.assertEqual(c.decode(body, c.MAX_RESPONSE)["answers"]["q"]["choice"], "opaque")
        self.assertEqual((receipt["requests"], receipt["responses"]), (1, 1))
        self.assertTrue(receipt["accepted_closed"] and receipt["body_completed"])
        self.assertFalse(receipt["cancel_requested"])
        self.assert_closed_fixture(receipt)

    def plan_fixture(self):
        wire = synthetic_wire()
        return wire, {"argv": ["/pinned-interpreter", "-I", "-S", "-B", "/pinned-helper"],
                      "cwd": Path("/synthetic-mechanism"), "env": {"LANG": "C"},
                      "header": {"hard_end": 60, "operation_end": 55, "wire_sha256": c.sha256(wire),
                                 "interpreter": "/pinned-interpreter", "interpreter_sha256": "a" * 64,
                                 "worker_sha256": "b" * 64, "contract_sha256": "c" * 64}}

    def test_prepared_interpreter_and_original_deadlines_remain_pinned(self):
        wire, plan = self.plan_fixture()
        factory = local.fixture_plan_factory(Path("/public-fixture-ca"), SimpleNamespace(port=43210), plan)
        with patch.object(t, "prepare_plan") as prepare, patch.object(t, "identity_hash", side_effect=[
                (Path("/pinned-interpreter"), "a" * 64), (Path("/pinned-helper"), "b" * 64),
                (Path("/pinned-contract"), "c" * 64)]) as identities:
            actual = factory(wire, 55, 60, Clock())
        prepare.assert_not_called()
        self.assertEqual(actual["argv"][:4], plan["argv"][:4])
        self.assertEqual(actual["header"], plan["header"])
        self.assertEqual(plan["argv"][-1], "/pinned-helper")
        self.assertEqual(identities.call_args_list[0].args[:2], ("/pinned-interpreter", 55))
        with patch.object(t, "identity_hash") as identities:
            with self.assertRaisesRegex(t.TransportFailure, "fixture_original_deadline"):
                factory(wire, 56, 61, Clock())
            identities.assert_not_called()

    def test_changed_prepared_interpreter_refuses_admission(self):
        wire, plan = self.plan_fixture()
        factory = local.fixture_plan_factory(Path("/public-fixture-ca"), SimpleNamespace(port=43210), plan)
        with patch.object(t, "identity_hash", return_value=(Path("/pinned-interpreter"), "d" * 64)):
            with self.assertRaisesRegex(t.TransportFailure, "trusted_file_changed"):
                factory(wire, 55, 60, Clock())
        self.assertEqual(plan["header"]["interpreter_sha256"], "a" * 64)

    def test_failed_cleanup_keeps_primary_transport_and_fixture_facts(self):
        original = {"error": "transport_tls", "created": True, "completion_confirmed": True}
        with ExitStack() as stack:
            fixture_factory, fixture_receipt = HarnessRegressionTests.harness_patches(self, stack)
            fixture_receipt.update(accepted_closed=False, thread_joined=False,
                                   cleanup_errors=["fixture_server_unconfirmed"])
            stack.enter_context(patch.object(t, "prepare_plan", return_value={
                "argv": ["/synthetic-interpreter", "-I", "-S", "-B", "/synthetic-helper"],
                "cwd": Path("/synthetic-mechanism"), "header": {"interpreter": "/synthetic-interpreter"}}))
            stack.enter_context(patch.object(local, "fixture_plan_factory", return_value=Mock()))
            remove = stack.enter_context(patch.object(t.shutil, "rmtree"))
            remove.side_effect = OSError("synthetic private-directory close failure")
            transport = stack.enter_context(patch.object(t, "JevTransport"))
            transport.return_value.send.side_effect = t.TransportFailure("transport_tls", original)
            output = io.StringIO()
            with redirect_stdout(output):
                self.assertEqual(local.main(), 1)
        report = json.loads(output.getvalue())
        fixture_factory.return_value.close.assert_called_once()
        self.assertEqual(report["error"], "transport_tls")
        self.assertEqual(report["transport"], original)
        self.assertEqual(report["fixture"], fixture_receipt)
        self.assertEqual(report["cleanup_errors"], ["fixture_plan_cleanup_os"])
        self.assertFalse(report["preparation"]["cwd_closed"])
        self.assertEqual(report["oracle_error"], "fixture_wrong_predicate")

    def test_certificate_preparation_failure_opens_no_listener_and_closes_plan(self):
        with ExitStack() as stack:
            fixture_factory, _ = HarnessRegressionTests.harness_patches(self, stack)
            stack.enter_context(patch.object(t, "prepare_plan", return_value={
                "cwd": Path("/synthetic-mechanism"), "header": {"interpreter": "/synthetic-interpreter"}}))
            context = local.ssl.SSLContext.return_value
            context.load_cert_chain.side_effect = local.ssl.SSLError("synthetic fixture certificate failure")
            remove = stack.enter_context(patch.object(t.shutil, "rmtree"))
            output = io.StringIO()
            with redirect_stdout(output):
                self.assertEqual(local.main(), 1)
        fixture_factory.assert_not_called()
        remove.assert_called_once_with(Path("/synthetic-mechanism"))
        report = json.loads(output.getvalue())
        self.assertEqual(report["error"], "SSLError")
        self.assertFalse(report["fixture"]["listener_opened"])
        self.assertTrue(report["preparation"]["cwd_closed"] and report["preparation"]["certificate_files_closed"])


class AbsoluteDeadlineHandoffTests(unittest.TestCase):
    def test_run_case_hands_off_captured_absolute_request_end(self):
        with ExitStack() as stack:
            _, fixture_receipt = HarnessRegressionTests.harness_patches(self, stack)
            stack.enter_context(patch.object(t, "prepare_plan", return_value={
                "argv": ["/synthetic-interpreter", "-I", "-S", "-B", "/synthetic-helper"],
                "cwd": Path("/synthetic-mechanism"), "header": {"interpreter": "/synthetic-interpreter"}}))
            stack.enter_context(patch.object(local, "fixture_plan_factory", return_value=Mock()))
            stack.enter_context(patch.object(t.shutil, "rmtree"))
            transport = stack.enter_context(patch.object(t, "JevTransport"))
            transport.return_value.send.side_effect = t.TransportFailure("synthetic_mechanism_refusal")
            report = local.run_case("success")
        send = transport.return_value.send
        send.assert_called_once()
        self.assertEqual(report["error"], "synthetic_mechanism_refusal")
        self.assertEqual(report["fixture"], fixture_receipt)
        self.assertEqual(send.call_args.kwargs.get("request_end"), report["hard_end"])
        self.assertEqual(send.call_args.kwargs["timeout"], t.REQUEST_SECONDS)
        self.assertEqual(transport.return_value.run_end, report["run_end"])

    def prepared_path(self, stack, clock, *, hard_end=60, run_end=600, complete=True):
        wire, plan = HarnessBoundaryTests.plan_fixture(self)
        plan["header"].update(hard_end=hard_end, operation_end=hard_end - 5)
        stack.enter_context(patch.object(t, "identity_hash", side_effect=lambda path, end, observed_clock: (
            Path("/pinned-interpreter") if str(path) == "/pinned-interpreter" else Path(path),
            "a" * 64 if str(path) == "/pinned-interpreter" else
            "b" * 64 if Path(path).name == "jev_request_worker.py" else "c" * 64)))
        factory = Mock(wraps=local.fixture_plan_factory(Path("/public-fixture-ca"), SimpleNamespace(port=43210), plan))
        leases = []
        def lease_factory(prepared, operation_end, actual_end, **kwargs):
            lease = Mock(facts={"start_finished": True, "error": None}, cancelled=False)
            answers = {"q": {"type": "choice", "choice": "x", "probabilities": {"x": 1}}}
            lease.stdout = c.encode({"version": 1, "status": "completed", "credential_channel_closed": True,
                                    "socket_closed": True, "wire_sha256": c.sha256(wire), "answers": answers,
                                    "tls": {"certificate_verified": True, "hostname_verified": True,
                                            "root_count": 1, "root_der_set_sha256": "d" * 64}})
            receipt = {key: True for key in ("start_finished", "created", "exit_observed", "reap_confirmed",
                       "stdout_complete", "stderr_complete", "stdout_closed", "stderr_closed",
                       "credential_channel_closed", "threads_joined", "handle_closed", "cwd_closed")}
            receipt.update(operation_end=operation_end, hard_end=actual_end, returncode=0,
                           completion_confirmed=complete, error=None if complete else "cleanup_unconfirmed")
            lease.finish.return_value = receipt
            lease.streams_and_exit_done.return_value = True
            def await_state(predicate, end):
                self.assertTrue(predicate(lease.facts))
                t.remaining(end, kwargs["clock"])
            lease.await_state.side_effect = await_state
            leases.append((lease, prepared, operation_end, actual_end))
            return lease
        admission = Mock(side_effect=lease_factory)
        transport = t.JevTransport(clock=clock, plan_factory=factory, lease_factory=admission)
        transport.run_end = run_end
        return SimpleNamespace(wire=wire, plan=plan, factory=factory, admission=admission,
                               transport=transport, leases=leases)

    def test_advancing_clock_reaches_real_send_and_original_prepared_path(self):
        with ExitStack() as stack:
            clock = Clock()
            path = self.prepared_path(stack, clock)
            clock.value = 2
            caller_remaining_timeout = 60 - clock()
            clock.value = 12
            result = path.transport.send(path.wire, token="synthetic-token", timeout=caller_remaining_timeout,
                                         request_end=60)
        path.factory.assert_called_once_with(path.wire, 55, 60, clock)
        path.admission.assert_called_once()
        lease, plan, operation_end, hard_end = path.leases[0]
        lease.admit.assert_called_once_with(path.wire, b"synthetic-token")
        self.assertEqual((operation_end, hard_end), (55, 60))
        self.assertEqual(plan["header"], path.plan["header"])
        self.assertEqual(plan["argv"][:4], path.plan["argv"][:4])
        self.assertEqual(path.transport.run_started, 0)
        self.assertEqual(path.transport.run_end, 600)
        self.assertEqual(result.receipt["hard_end"], 60)
        self.assertEqual(result.receipt["interpreter_sha256"], "a" * 64)
        self.assertEqual(result.receipt["wire_sha256"], c.sha256(path.wire))
        self.assertEqual(result.payload["answers"]["q"]["choice"], "x")

    def assert_deadline_refused_before_admission(self, now):
        clock = Clock()
        factory, lease_factory = Mock(), Mock()
        transport = t.JevTransport(clock=clock, plan_factory=factory, lease_factory=lease_factory)
        clock.value = now
        with self.assertRaisesRegex(t.TransportFailure, "run_deadline") as caught:
            transport.send(synthetic_wire(), token="synthetic-token", request_end=60)
        factory.assert_not_called()
        lease_factory.assert_not_called()
        self.assertEqual(transport.calls, 0)
        self.assertEqual(transport.retained, [])
        receipt = caught.exception.receipt
        self.assertEqual(receipt["phase"], "admission")
        self.assertEqual(receipt["request_end"], 60)
        self.assertEqual(receipt["run_end"], 600)
        self.assertFalse(receipt["process_admitted"] or receipt["completion_confirmed"])

    def test_expired_original_request_end_refuses_before_plan_lease_process(self):
        self.assert_deadline_refused_before_admission(61)

    def test_consumed_operation_window_refuses_with_cleanup_reserve_remaining(self):
        for now in (55, 56):
            with self.subTest(now=now):
                self.assert_deadline_refused_before_admission(now)

    def test_earlier_run_or_timeout_clamp_never_extends_or_rewrites_prepared_end(self):
        for run_end, timeout, effective_end in ((40, 60, 40), (600, 15, 27)):
            with self.subTest(run_end=run_end, timeout=timeout), ExitStack() as stack:
                clock = Clock()
                path = self.prepared_path(stack, clock, run_end=run_end)
                clock.value = 12
                with self.assertRaisesRegex(t.TransportFailure, "fixture_original_deadline") as caught:
                    path.transport.send(path.wire, token="synthetic-token", timeout=timeout, request_end=60)
                path.factory.assert_called_once_with(path.wire, effective_end - 5, effective_end, clock)
                path.admission.assert_not_called()
                self.assertEqual(caught.exception.receipt["hard_end"], effective_end)
                self.assertEqual(path.plan["header"]["hard_end"], 60)
                self.assertEqual(path.transport.run_end, run_end)
                self.assertEqual(path.transport.retained, [])

    def test_invalid_absolute_bounds_refuse_before_any_admission(self):
        for bound in (True, False, float("nan"), float("inf"), -float("inf"), "60", [], 1 << 4096):
            with self.subTest(bound_type=type(bound).__name__):
                factory, lease_factory = Mock(), Mock()
                transport = t.JevTransport(clock=Clock(), plan_factory=factory, lease_factory=lease_factory)
                with self.assertRaisesRegex(t.TransportFailure, "request_end_bound") as caught:
                    transport.send(synthetic_wire(), token="synthetic-token", request_end=bound)
                factory.assert_not_called()
                lease_factory.assert_not_called()
                self.assertFalse(caught.exception.receipt["process_admitted"])
                self.assertEqual(transport.calls, 0)
                self.assertNotIn("request_end", caught.exception.receipt)

    def test_sub_microsecond_later_prepared_end_is_refused_exactly(self):
        for field in ("hard_end", "operation_end"):
            with self.subTest(field=field), ExitStack() as stack:
                clock = Clock()
                path = self.prepared_path(stack, clock)
                path.plan["header"][field] += 0.0000001
                clock.value = 12
                with self.assertRaisesRegex(t.TransportFailure, "fixture_original_deadline"):
                    path.transport.send(path.wire, token="synthetic-token", request_end=60)
                path.admission.assert_not_called()
                self.assertGreater(path.plan["header"][field], 60 if field == "hard_end" else 55)

    def test_absent_absolute_bound_preserves_sixty_six_hundred_policy(self):
        for now, end in ((0, 60), (570, 600)):
            with self.subTest(now=now), ExitStack() as stack:
                clock = Clock()
                path = self.prepared_path(stack, clock, hard_end=end)
                clock.value = now
                result = path.transport.send(path.wire, token="synthetic-token")
                self.assertEqual((result.receipt["operation_end"], result.receipt["hard_end"]), (end - 5, end))
                self.assertEqual(path.transport.run_end, 600)
                path.admission.assert_called_once()
                path.leases[0][0].admit.assert_called_once_with(path.wire, b"synthetic-token")

    def test_expired_harness_admission_keeps_original_failure_and_fixture_cleanup(self):
        for now in (56, 61):
            with self.subTest(now=now), ExitStack() as stack:
                fixture_factory, fixture_receipt = HarnessRegressionTests.harness_patches(self, stack)
                clock = Clock()
                stack.enter_context(patch.object(local.time, "monotonic", clock))
                stack.enter_context(patch.object(t, "prepare_plan", return_value={
                    "cwd": Path("/synthetic-mechanism"), "header": {"interpreter": "/synthetic-interpreter"}}))
                factory = Mock()
                stack.enter_context(patch.object(local, "fixture_plan_factory", return_value=factory))
                stack.enter_context(patch.object(t.shutil, "rmtree"))
                real_transport = t.JevTransport
                stack.enter_context(patch.object(t, "JevTransport", side_effect=lambda **kwargs:
                                                real_transport(clock=clock, **kwargs)))
                lease_factory = stack.enter_context(patch.object(t, "OwnedLease"))
                fixture_factory.return_value.start.side_effect = lambda: setattr(clock, "value", now)
                report = local.run_case("success")
            factory.assert_not_called()
            lease_factory.assert_not_called()
            self.assertEqual(report["error"], "run_deadline")
            self.assertEqual(report["transport"]["error"], "run_deadline")
            self.assertEqual(report["transport"]["request_end"], report["hard_end"])
            self.assertFalse(report["transport"]["process_admitted"])
            self.assertEqual(report["fixture"], fixture_receipt)
            self.assertEqual(report["oracle_error"], "fixture_wrong_predicate")

    def test_poisoned_second_call_retains_absolute_end_and_refuses_before_admission(self):
        with ExitStack() as stack:
            clock = Clock()
            path = self.prepared_path(stack, clock, complete=False)
            clock.value = 12
            with self.assertRaisesRegex(t.TransportFailure, "cleanup_unconfirmed"):
                path.transport.send(path.wire, token="synthetic-token", request_end=60)
            clock.value = 61
            with self.assertRaisesRegex(t.TransportFailure, "lease_unavailable"):
                path.transport.send(path.wire, token="synthetic-token", request_end=60)
            path.factory.assert_called_once()
            path.admission.assert_called_once()
        with ExitStack() as stack:
            HarnessRegressionTests.harness_patches(self, stack)
            stack.enter_context(patch.object(t, "prepare_plan", return_value={
                "cwd": Path("/synthetic-mechanism"), "header": {"interpreter": "/synthetic-interpreter"}}))
            stack.enter_context(patch.object(local, "fixture_plan_factory", return_value=Mock()))
            stack.enter_context(patch.object(t.shutil, "rmtree"))
            stack.enter_context(patch.object(local, "oracle_error", return_value=None))
            transport = stack.enter_context(patch.object(t, "JevTransport"))
            transport.return_value.send.side_effect = [t.TransportFailure("cleanup_unconfirmed"),
                                                      t.TransportFailure("lease_unavailable")]
            report = local.run_case("partial_capture_failure")
        self.assertEqual(transport.return_value.send.call_count, 2)
        for call in transport.return_value.send.call_args_list:
            self.assertEqual(call.kwargs["request_end"], report["hard_end"])
            self.assertEqual(call.kwargs["timeout"], t.REQUEST_SECONDS)


if __name__ == "__main__":
    unittest.main()
