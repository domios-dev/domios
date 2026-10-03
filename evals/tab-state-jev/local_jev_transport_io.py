#!/usr/bin/env python3
"""WHAT: actual production helper against one owned loopback TLS listener.
WHY: pure mechanism controls cannot establish native exit/stream/handle closure.
ORACLE: exact request/one response and owned completion receipts for every case.
Public fixture EC key d=1 is intentionally public; no real token/config is read.
"""

import argparse
import base64
import errno
import hashlib
import json
import os
from pathlib import Path
import selectors
import socket
import ssl
import sys
import tempfile
import threading
import time

import jev_transport as t
import request_contract as c
from windows_junction_fixture import create_junction

CASES = ("success", "response_oversize", "redirect_refused", "held_response_deadline",
         "cancel_after_headers", "partial_capture_failure")
SYNTHETIC_TOKEN = "synthetic-loopback-mechanism-token"


def der(tag, raw):
    length = len(raw)
    length_raw = bytes([length]) if length < 128 else length.to_bytes((length.bit_length() + 7) // 8, "big")
    prefix = b"" if length < 128 else bytes([0x80 | len(length_raw)])
    return bytes([tag]) + prefix + length_raw + raw


def sequence(*items):
    return der(0x30, b"".join(items))


def integer(value):
    raw = value.to_bytes(max(1, (value.bit_length() + 7) // 8), "big")
    return der(2, b"\0" + raw if raw[0] & 0x80 else raw)


def oid(*parts):
    raw = bytes([40 * parts[0] + parts[1]])
    for value in parts[2:]:
        octets = [value & 127]
        value >>= 7
        while value:
            octets.insert(0, 128 | (value & 127))
            value >>= 7
        raw += bytes(octets)
    return der(6, raw)


def pem(kind, raw):
    encoded = base64.b64encode(raw)
    lines = [encoded[index:index + 64] for index in range(0, len(encoded), 64)]
    return b"-----BEGIN " + kind + b"-----\n" + b"\n".join(lines) + b"\n-----END " + kind + b"-----\n"


def public_fixture_certificate():
    prime = 0xffffffff00000001000000000000000000000000ffffffffffffffffffffffff
    order = 0xffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551
    gx = 0x6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296
    gy = 0x4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5
    point = b"\4" + gx.to_bytes(32, "big") + gy.to_bytes(32, "big")
    curve = oid(1, 2, 840, 10045, 3, 1, 7)
    algorithm = sequence(oid(1, 2, 840, 10045, 4, 3, 2))
    name = sequence(der(0x31, sequence(oid(2, 5, 4, 3), der(0x0c, b"Public loopback mechanism fixture"))))
    public_key = sequence(sequence(oid(1, 2, 840, 10045, 2, 1), curve), der(3, b"\0" + point))
    constraints = sequence(oid(2, 5, 29, 19), der(1, b"\xff"), der(4, sequence(der(1, b"\xff"))))
    san = sequence(oid(2, 5, 29, 17), der(4, sequence(der(0x87, socket.inet_aton("127.0.0.1")))))
    validity = sequence(der(0x17, b"260101000000Z"), der(0x17, b"360101000000Z"))
    body = sequence(der(0xa0, integer(2)), integer(1), algorithm, name, validity, name, public_key,
                    der(0xa3, sequence(constraints, san)))
    slope = ((3 * gx * gx - 3) * pow(2 * gy, -1, prime)) % prime
    r = (slope * slope - 2 * gx) % prime % order
    digest = int.from_bytes(hashlib.sha256(body).digest(), "big")
    signature = sequence(integer(r), integer(((digest + r) * pow(2, -1, order)) % order))
    certificate = pem(b"CERTIFICATE", sequence(body, algorithm, der(3, b"\0" + signature)))
    key = pem(b"EC PRIVATE KEY", sequence(integer(1), der(4, (1).to_bytes(32, "big")),
                                         der(0xa0, curve), der(0xa1, der(3, b"\0" + point))))
    return certificate, key


def event_wait(event, end, code):
    if not event.wait(t.remaining(end, time.monotonic)):
        raise t.TransportFailure(code)
    t.remaining(end, time.monotonic)


def read_request(stream, end):
    raw = b""
    while b"\r\n\r\n" not in raw:
        stream.settimeout(t.remaining(end, time.monotonic))
        data = stream.recv(4096)
        if not data:
            raise t.TransportFailure("fixture_request_incomplete")
        raw += data
        if len(raw) > c.MAX_REQUEST + 16384:
            raise t.TransportFailure("fixture_request_size")
    headers, body = raw.split(b"\r\n\r\n", 1)
    fields = {}
    for line in headers.split(b"\r\n")[1:]:
        key, value = line.split(b":", 1)
        fields[key.lower()] = value.strip()
    if fields.get(b"authorization") != b"Bearer " + SYNTHETIC_TOKEN.encode():
        raise t.TransportFailure("fixture_credential_mismatch")
    length = int(fields[b"content-length"])
    if length > c.MAX_REQUEST:
        raise t.TransportFailure("fixture_request_size")
    while len(body) < length:
        stream.settimeout(t.remaining(end, time.monotonic))
        data = stream.recv(min(4096, length - len(body)))
        if not data:
            raise t.TransportFailure("fixture_request_incomplete")
        body += data
    if len(body) != length:
        raise t.TransportFailure("fixture_request_size")
    return body


class LoopbackFixture:
    def __init__(self, case, context, operation_end, hard_end):
        self.case, self.context = case, context
        self.operation_end, self.hard_end = operation_end, hard_end
        self.ready, self.headers_sent, self.release, self.done, self.cancelled = (threading.Event() for _ in range(5))
        self.lock = threading.RLock()
        self.condition = threading.Condition(self.lock)
        self.requests, self.responses, self.request_bytes = 0, 0, None
        self.body_completed, self.response_bytes_sha256 = False, None
        self.error, self.cleanup_errors = None, []
        self.accepted = None
        self.accept_observed, self.accepted_closed = False, False
        self.listener_closed, self.selector_closed = False, False
        self.wake_reader_closed, self.wake_writer_closed = False, False
        self.wake_sent, self.wake_observed, self.cancel_observed = False, False, False
        self.cancellation_phase, self.thread_started, self.thread_joined = None, False, False
        self.listener = self.wake_reader = self.wake_writer = self.selector = None
        try:
            t.remaining(operation_end, time.monotonic)
            self.wake_reader, self.wake_writer = socket.socketpair()
            self.wake_writer.setblocking(False)
            self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            self.listener.bind(("127.0.0.1", 0))
            self.port = self.listener.getsockname()[1]
            self.listener.listen(1)
            self.listener.setblocking(False)
            self.selector = selectors.DefaultSelector()
            self.selector.register(self.listener, selectors.EVENT_READ, "listener")
            self.selector.register(self.wake_reader, selectors.EVENT_READ, "cancel")
            self.thread = threading.Thread(target=self.serve, name="jev-fixture-server", daemon=False)
            t.remaining(operation_end, time.monotonic)
        except BaseException as error:
            self.retire()
            self.close_resource("wake_writer", "wake_writer_closed")
            raise t.TransportFailure(getattr(error, "code", "fixture_creation_os"), self.snapshot()) from error

    def start(self):
        self.thread.start()
        self.thread_started = True
        event_wait(self.ready, self.operation_end, "fixture_not_ready")

    def request_cancel(self):
        with self.condition:
            if self.done.is_set():
                return
            self.cancelled.set()
            self.release.set()
            if not self.wake_sent:
                try:
                    self.wake_sent = self.wake_writer.send(b"C") == 1
                    if not self.wake_sent:
                        self.cleanup_errors.append("fixture_wake_unconfirmed")
                except OSError:
                    self.cleanup_errors.append("fixture_wake_os")
            if self.accepted is not None and not self.accepted_closed:
                try:
                    self.accepted.shutdown(socket.SHUT_RDWR)
                except OSError as error:
                    if error.errno != errno.ENOTCONN:
                        self.cleanup_errors.append("fixture_peer_shutdown_os")
            self.condition.notify_all()

    def accept_or_cancel(self):
        self.ready.set()
        events = self.selector.select(t.remaining(self.operation_end, time.monotonic))
        t.remaining(self.operation_end, time.monotonic)
        if any(key.data == "cancel" for key, _ in events):
            self.wake_observed = self.wake_reader.recv(1) == b"C"
            t.require(self.wake_observed and self.cancelled.is_set(), "fixture_wake_shape")
        with self.lock:
            if self.cancelled.is_set():
                self.cancel_observed, self.cancellation_phase = True, "pre_accept"
                raise t.TransportFailure("fixture_cancelled")
            t.require(any(key.data == "listener" for key, _ in events), "fixture_accept_deadline")
            plain, _ = self.listener.accept()
            self.accepted, self.accept_observed = plain, True
            plain.settimeout(t.remaining(self.operation_end, time.monotonic))
            # Publish TLS authority before blocking handshake so cancellation owns its wake.
            self.accepted = self.context.wrap_socket(plain, server_side=True, do_handshake_on_connect=False)
            return self.accepted

    def serve(self):
        try:
            stream = self.accept_or_cancel()
            stream.do_handshake()
            with self.lock:
                t.require(not self.cancelled.is_set(), "fixture_cancelled")
            self.request_bytes = read_request(stream, self.operation_end)
            self.requests += 1
            body = c.encode({"answers": {"q": {"type": "choice", "choice": "opaque",
                                               "probabilities": {"opaque": 0.95}}}})
            self.response_bytes_sha256 = c.sha256(body)
            status, extra = b"200 OK", b""
            if self.case == "response_oversize":
                body += b" " * 65537
            elif self.case == "redirect_refused":
                status, extra, body = b"302 Found", b"Location: https://elsewhere.invalid/\r\n", b""
            headers = b"HTTP/1.1 " + status + b"\r\n" + extra + b"Content-Length: " + str(len(body)).encode() + \
                b"\r\nConnection: close\r\n\r\n"
            stream.settimeout(t.remaining(self.operation_end, time.monotonic))
            stream.sendall(headers)
            with self.condition:
                self.headers_sent.set()
                self.responses += 1
                self.condition.notify_all()
            if self.case in ("held_response_deadline", "cancel_after_headers"):
                event_wait(self.release, self.hard_end, "fixture_hold_deadline")
                self.cancel_observed = self.cancelled.is_set()
                self.cancellation_phase = "accepted" if self.cancel_observed else None
            else:
                for offset in range(0, len(body), 4096):
                    stream.settimeout(t.remaining(self.operation_end, time.monotonic))
                    stream.sendall(body[offset:offset + 4096])
                self.body_completed = True
        except BaseException as error:
            if self.cancelled.is_set() and (isinstance(error, OSError) or
                    getattr(error, "code", None) == "fixture_cancelled"):
                self.cancel_observed = True
                self.cancellation_phase = "accepted" if self.accept_observed else "pre_accept"
            elif not (self.case in ("response_oversize", "redirect_refused") and isinstance(error, OSError)):
                self.error = getattr(error, "code", type(error).__name__)
        finally:
            with self.condition:
                self.retire()
                self.done.set()
                self.condition.notify_all()

    def close_resource(self, name, fact):
        with self.lock:
            resource = getattr(self, name)
            if resource is not None and not getattr(self, fact):
                try:
                    resource.close()
                    setattr(self, fact, True)
                except BaseException:
                    self.cleanup_errors.append("fixture_" + name + "_close")

    def retire(self):
        for name, fact in (("accepted", "accepted_closed"), ("listener", "listener_closed"),
                           ("selector", "selector_closed"), ("wake_reader", "wake_reader_closed")):
            self.close_resource(name, fact)

    def wait_for_headers(self):
        with self.condition:
            while not self.headers_sent.is_set() and not self.done.is_set() and not self.cancelled.is_set():
                self.condition.wait(t.remaining(self.operation_end, time.monotonic))
            t.require(self.headers_sent.is_set(), "fixture_headers_unconfirmed")
            t.remaining(self.operation_end, time.monotonic)

    def snapshot(self):
        return {"listener_opened": self.listener is not None, "requests": self.requests, "responses": self.responses,
                "accept_observed": self.accept_observed, "accepted_closed": self.accepted_closed,
                "listener_closed": self.listener_closed, "selector_closed": self.selector_closed,
                "wake_reader_closed": self.wake_reader_closed, "wake_writer_closed": self.wake_writer_closed,
                "thread_started": self.thread_started, "thread_joined": self.thread_joined,
                "retirement_observed": self.done.is_set(), "cancel_requested": self.cancelled.is_set(),
                "wake_sent": self.wake_sent, "wake_observed": self.wake_observed,
                "cancel_observed": self.cancel_observed, "cancellation_phase": self.cancellation_phase,
                "body_completed": self.body_completed, "error": self.error,
                "cleanup_errors": list(self.cleanup_errors)}

    def close(self):
        self.request_cancel()
        try:
            if self.thread_started:
                event_wait(self.done, self.hard_end, "fixture_server_unconfirmed")
                self.thread.join(t.remaining(self.hard_end, time.monotonic))
                t.require(not self.thread.is_alive(), "fixture_thread_unconfirmed")
                self.thread_joined = True
            else:
                self.retire()
                self.done.set()
            self.close_resource("wake_writer", "wake_writer_closed")
            t.remaining(self.hard_end, time.monotonic)
        except BaseException as error:
            self.cleanup_errors.append(getattr(error, "code", "fixture_cleanup_os"))
        return self.snapshot()


class PartialCapture:
    def __init__(self, stream):
        self.stream, self.read_once = stream, False

    def read(self, size):
        if self.read_once:
            raise OSError("synthetic partial-capture mechanism failure")
        self.read_once = True
        raw = self.stream.read(size)
        if not raw:
            raise OSError("synthetic capture did not supply a partial chunk")
        return raw


def fixture_plan_factory(ca_path, fixture, prepared_plan):
    def build(wire, operation_end, hard_end, clock):
        plan = prepared_plan
        t.require(plan["header"]["hard_end"] == hard_end and
                  plan["header"]["operation_end"] == operation_end and
                  plan["header"]["wire_sha256"] == c.sha256(wire), "fixture_original_deadline")
        for path, field in ((plan["argv"][0], "interpreter_sha256"),
                            (Path(__file__).with_name("jev_request_worker.py"), "worker_sha256"),
                            (Path(__file__).with_name("request_contract.py"), "contract_sha256")):
            exact, digest = t.identity_hash(path, operation_end, clock)
            t.require(digest == plan["header"][field], "trusted_file_changed")
            if field == "interpreter_sha256":
                t.require(str(exact) == plan["header"]["interpreter"], "trusted_file_changed")
        worker = Path(__file__).with_name("jev_request_worker.py").resolve()
        # Only this harness builds the private in-process policy; production has no switch or allowlist.
        bootstrap = "\n".join([
            "import importlib.util, ssl, sys",
            f"path = {str(worker)!r}",
            "spec = importlib.util.spec_from_file_location('jev_request_worker', path)",
            "worker = importlib.util.module_from_spec(spec)",
            "spec.loader.exec_module(worker)",
            "sys.argv = [path]",
            f"context = ssl.create_default_context(cafile={str(ca_path)!r})",
            f"policy = {{'endpoint': 'https://127.0.0.1:{fixture.port}/v1/systemone', 'port': {fixture.port}, 'context': context}}",
            "sys.exit(worker.main(policy=policy))",
        ])
        return {**plan, "argv": plan["argv"][:4] + ["-c", bootstrap]}
    return build


def unstarted_fixture_receipt():
    return {"listener_opened": False, "requests": 0, "responses": 0, "accept_observed": False,
            "accepted_closed": False, "listener_closed": False, "thread_started": False,
            "thread_joined": False, "retirement_observed": False, "error": None, "cleanup_errors": []}


def oracle_error(case, outcome, receipt, fixture, fixture_receipt, cancel_errors, transport, wire):
    expected = {"success": {"completed"}, "response_oversize": {"response_oversize"},
                "redirect_refused": {"redirect_refused"}, "held_response_deadline": {"deadline"},
                "cancel_after_headers": {"cancelled"}, "partial_capture_failure": {"cleanup_unconfirmed"}}[case]
    if outcome not in expected:
        return "fixture_wrong_predicate"
    if not all(receipt.get(key) is True for key in (
            "created", "exit_observed", "reap_confirmed", "stdout_closed", "stderr_closed",
            "credential_channel_closed", "threads_joined", "handle_closed", "cwd_closed")):
        return "fixture_native_completion_unconfirmed"
    if receipt.get("windows_trust_required") and receipt.get("trust_handles_closed") is not True:
        return "fixture_native_trust_unconfirmed"
    if fixture is None or not all(fixture_receipt.get(key) is True for key in (
            "accept_observed", "accepted_closed", "listener_closed", "selector_closed", "wake_reader_closed",
            "wake_writer_closed", "retirement_observed", "thread_joined")) or \
            fixture_receipt["error"] is not None or fixture_receipt["cleanup_errors"] or cancel_errors:
        return "fixture_resource_unconfirmed"
    if fixture.request_bytes != wire or fixture.requests != 1 or fixture.responses != 1:
        return "fixture_wire_or_count"
    if case == "success" and (not fixture.body_completed or
            receipt.get("helper", {}).get("response_bytes_sha256") != fixture.response_bytes_sha256):
        return "fixture_response_completion"
    if case == "partial_capture_failure":
        if receipt["completion_confirmed"] or not transport.poisoned or receipt["stdout_bytes"] <= 0 or \
                not fixture.body_completed:
            return "fixture_partial_capture_laundered"
    elif receipt["completion_confirmed"] is not True:
        return "fixture_capture_completion_unconfirmed"
    return None


def run_case(case):
    started = time.monotonic()
    run_end = started + 600
    hard_end = min(started + 60, run_end)
    operation_end = hard_end - 5
    report = {"case": case, "oracle": "failed", "platform": sys.platform,
              "interpreter": sys.version.split()[0], "selected_interpreter": str(Path(sys.executable).resolve()),
              "fixture_type": "synthetic_mechanism", "case_started": started,
              "operation_end": operation_end, "hard_end": hard_end, "run_end": run_end,
              "transport": {}, "fixture": unstarted_fixture_receipt(), "cleanup_errors": [],
              "preparation": {"plan_created": False, "lease_created": False, "cwd_closed": False,
                              "certificate_files_closed": False},
              "expected_requests": 1, "expected_responses": 1}
    fixture = cancel_thread = transport = temporary = prepared_plan = wire = None
    lease_created, cancel_errors = False, []
    outcome, receipt = None, {}
    phase = "preparation"
    try:
        t.native_support()
        wire = c.encode({"state": "synthetic loopback mechanism fixture", "model": "fixture-alias", "questions": {
            "q": {"type": "choice", "instructions": "opaque", "criteria": {"opaque": "opaque"}}}})
        c.validate_wire(wire)
        t.token_bytes(SYNTHETIC_TOKEN)
        prepared_plan = t.prepare_plan(wire, operation_end, hard_end, time.monotonic)
        report["preparation"]["plan_created"] = True
        if "windows_guard" in prepared_plan:
            report["preparation"]["windows_trust"] = prepared_plan["windows_guard"].receipt()
        report["selected_interpreter"] = prepared_plan["header"]["interpreter"]
        report["python_wire_bytes_sha256"] = c.sha256(wire)
        temporary = tempfile.TemporaryDirectory(prefix="jev-public-tls-fixture-")
        root = Path(temporary.__enter__())
        certificate, key = public_fixture_certificate()
        ca_path, key_path = root / "public-test-ca.pem", root / "public-test-key.pem"
        ca_path.write_bytes(certificate)
        key_path.write_bytes(key)
        report["public_test_ca_bytes_sha256"] = c.sha256(certificate)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(str(ca_path), str(key_path))
        t.remaining(operation_end, time.monotonic)
        phase = "fixture"
        fixture = LoopbackFixture(case, context, operation_end, hard_end)
        fixture.start()
        def lease_factory(plan, operation_end, hard_end, **kwargs):
            nonlocal lease_created
            source = (lambda name, stream: PartialCapture(stream) if name == "stdout" else stream) \
                if case == "partial_capture_failure" else None
            lease = t.OwnedLease(plan, operation_end, hard_end, capture_source=source, **kwargs)
            lease_created = True
            report["preparation"]["lease_created"] = True
            return lease
        transport = t.JevTransport(plan_factory=fixture_plan_factory(ca_path, fixture, prepared_plan),
                                   lease_factory=lease_factory)
        transport.run_started, transport.run_end = started, run_end
        if case == "cancel_after_headers":
            def cancel_on_headers():
                try:
                    fixture.wait_for_headers()
                    transport.cancel()
                except BaseException as error:
                    cancel_errors.append(getattr(error, "code", type(error).__name__))
            cancel_thread = threading.Thread(target=cancel_on_headers, name="jev-fixture-cancel", daemon=False)
            cancel_thread.start()
        phase = "transport"
        result = transport.send(wire, token=SYNTHETIC_TOKEN, timeout=t.REQUEST_SECONDS, request_end=hard_end)
        receipt, outcome = result.receipt, "completed"
        if case != "success" or result.payload["answers"]["q"]["choice"] != "opaque":
            outcome = "fixture_unexpected_success"
    except BaseException as error:
        outcome = getattr(error, "code", type(error).__name__)
        receipt = getattr(error, "receipt", {})
        if phase == "preparation":
            receipt = {**receipt, "error": outcome, "phase": "preparation", "process_admitted": False,
                       "completion_confirmed": False, "operation_end": operation_end, "hard_end": hard_end,
                       "run_end": run_end}
        elif phase == "fixture" and fixture is None and receipt:
            report["fixture"] = receipt
            receipt = {}
    finally:
        if fixture is not None:
            try:
                report["fixture"] = fixture.close()
            except BaseException as error:
                report["fixture"] = fixture.snapshot()
                report["cleanup_errors"].append(getattr(error, "code", "fixture_cleanup_os"))
        if cancel_thread is not None:
            try:
                cancel_thread.join(t.remaining(hard_end, time.monotonic))
                t.require(not cancel_thread.is_alive(), "fixture_cancel_thread_unconfirmed")
            except BaseException as error:
                report["cleanup_errors"].append(getattr(error, "code", "fixture_cancel_cleanup_os"))
        report["cancel_thread"] = {"created": cancel_thread is not None,
                                   "joined": cancel_thread is not None and not cancel_thread.is_alive(),
                                   "errors": cancel_errors}
        if prepared_plan is not None and not lease_created:
            try:
                if "windows_guard" in prepared_plan:
                    prepared_plan["windows_guard"].close(remove_cwd=True, cleanup_end=hard_end)
                else:
                    t.shutil.rmtree(prepared_plan["cwd"])
                report["preparation"]["cwd_closed"] = True
                t.remaining(hard_end, time.monotonic)
            except BaseException as error:
                report["cleanup_errors"].append(getattr(error, "code", "fixture_plan_cleanup_os"))
        elif lease_created:
            report["preparation"]["cwd_closed"] = receipt.get("cwd_closed") is True
        if prepared_plan is not None and "windows_guard" in prepared_plan:
            report["preparation"]["windows_trust"] = prepared_plan["windows_guard"].receipt()
        if temporary is not None:
            try:
                temporary.cleanup()
                report["preparation"]["certificate_files_closed"] = True
                t.remaining(hard_end, time.monotonic)
            except BaseException as error:
                report["cleanup_errors"].append(getattr(error, "code", "fixture_files_cleanup_os"))
    report.update(outcome=outcome, transport=receipt)
    try:
        predicate = oracle_error(case, outcome, receipt, fixture, report["fixture"], cancel_errors, transport, wire)
    except BaseException:
        predicate = "fixture_oracle_schema"
    if case == "partial_capture_failure" and predicate is None:
        try:
            transport.send(wire, token=SYNTHETIC_TOKEN, timeout=t.REQUEST_SECONDS, request_end=hard_end)
        except BaseException as error:
            if getattr(error, "code", None) != "lease_unavailable":
                predicate = "fixture_unknown_completion_admitted_request"
        else:
            predicate = "fixture_unknown_completion_admitted_request"
    if report["cleanup_errors"] and predicate is None:
        predicate = "fixture_cleanup_unconfirmed"
    if time.monotonic() >= hard_end and predicate is None:
        predicate = "fixture_deadline"
    report["oracle_error"] = predicate
    report["oracle"] = "satisfied" if predicate is None else "failed"
    report["error"] = None if predicate is None else outcome if outcome != "completed" else predicate
    report["elapsed_seconds"] = time.monotonic() - started
    return report


TRUST_CASES = ("preflight", "trusted_directory", "foreign_write", "null_dacl", "inherited_write", "reparse_refused")


def run_trust_case(case):
    started = time.monotonic()
    run_end, hard_end = started + 600, started + 60
    operation_end = hard_end - 5
    report = {"trust_case": case, "platform": sys.platform, "oracle": "failed", "error": None,
              "fixture_type": "synthetic_mechanism", "selected_interpreter": sys.executable,
              "case_started": started, "operation_end": operation_end, "hard_end": hard_end, "run_end": run_end,
              "helper_count": 0, "requests": 0, "responses": 0, "listener_opened": False,
              "cleanup_errors": [], "runtime": {}, "fixture": {}, "native_control": "NOT RUN"}
    plan = guard = probe = None
    paths, partial_owners, predicate = [], [], None
    expected = {"preflight": None, "trusted_directory": None, "foreign_write": "windows_foreign_authority",
                "null_dacl": "windows_dacl", "inherited_write": "windows_foreign_authority",
                "reparse_refused": "windows_reparse_or_links"}[case]

    def created(path, descriptor=None, *, target=None, inherit=False):
        node = {"path": path, "reparse": target is not None, "creation_confirmed": False,
                "entry": None, "identity_confirmed": False, "removed": False}
        paths.append(node)
        t.remaining(operation_end, time.monotonic)
        if inherit:
            os.mkdir(path)
        elif target is None:
            guard.api.create_directory(path, descriptor)
        else:
            try:
                report["fixture"]["reparse"] = create_junction(guard, path, target, descriptor)
            except OSError as error:
                facts = {name: value for name in ("errno", "winerror")
                         if type(value := getattr(error, name, None)) is int and -(2 ** 31) <= value < 2 ** 32}
                raise t.TransportFailure("windows_reparse_control_unavailable", {"reparse_creation_os": facts}) from error
        node["creation_confirmed"] = True
        entry = guard.capture(path, directory=True)
        node["entry"] = entry
        facts = guard.api.identify(entry["handle"]) if target is not None else guard.api.inspect(entry["handle"])
        entry["facts"] = facts
        t.require(facts["directory"] and os.path.normcase(facts["final"]) == os.path.normcase(str(path)),
                  "windows_fixture_identity")
        node["identity_confirmed"] = True
        return node

    try:
        t.require(t.native_support() == "nt", "windows_native_required")
        wire = c.encode({"state": "synthetic Windows admission fixture", "model": "fixture-alias", "questions": {
            "q": {"type": "choice", "instructions": "opaque", "criteria": {"opaque": "opaque"}}}})
        plan = t.prepare_plan(wire, operation_end, hard_end, time.monotonic)
        report["selected_interpreter"] = plan["header"]["interpreter"]
        report["runtime"] = {"implementation": sys.implementation.name, "version": sys.version,
                             "architecture": "win-amd64", "interpreter_sha256": plan["header"]["interpreter_sha256"],
                             "helper_sha256": plan["header"]["worker_sha256"],
                             "contract_sha256": plan["header"]["contract_sha256"],
                             "subprocess_sha256": t.worker_support.WINDOWS_SUBPROCESS_SHA256,
                             "trust": plan["windows_guard"].receipt()}
        plan["windows_guard"].close(remove_cwd=True, cleanup_end=hard_end)
        report["runtime"].update(trust_handles_closed=plan["windows_guard"].closed,
                                 cwd_closed=plan["windows_guard"].created_directory is None)
        if case != "preflight":
            guard = t.worker_support.WindowsTrust(operation_end)
            root = guard.create_private()
            user = guard.user
            child = root / "control"
            secure = f"O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"
            descriptor = secure
            if case == "foreign_write":
                descriptor += "(A;;FW;;;WD)"
            elif case == "null_dacl":
                descriptor = f"O:{user}D:NO_ACCESS_CONTROL"
            elif case == "inherited_write":
                inheriting = root / "inheriting"
                parent_control = created(inheriting, secure + "(A;OICIIO;FW;;;WD)")
                report["fixture"]["inheritance_parent_security"] = parent_control["entry"]["facts"]["security"]["aces"]
                child = inheriting / "control"
            if case == "reparse_refused":
                target = root / "target"
                created(target, secure)
                control = created(child, secure, target=target)
                t.require(control["entry"]["facts"]["identity"].hex() == report["fixture"]["reparse"]["identity"],
                          "windows_fixture_identity")
                t.require(control["entry"]["facts"]["attributes"] & 0x400, "windows_reparse_control_unavailable")
            else:
                control = created(child, descriptor, inherit=case == "inherited_write")
                if case == "inherited_write":
                    report["fixture"]["inheritance_child_security"] = control["entry"]["facts"]["security"]["aces"]
                    t.require(any(kind == 0 and flags & 0x10 and sid == "S-1-1-0" and mask & 2
                                  for kind, flags, mask, sid in control["entry"]["facts"]["security"]["aces"]),
                              "windows_inherited_control_unavailable")
            probe = t.worker_support.WindowsTrust(operation_end)
            try:
                probe.directory(child)
            except t.worker_support.WorkerError as error:
                predicate = error.code
            report["native_control"] = "EXECUTED"
            report["fixture"].update(observed_predicate=predicate, expected_predicate=expected)
        else:
            report["native_control"] = "EXECUTED"
        t.require(predicate == expected, "windows_trust_wrong_predicate")
    except BaseException as error:
        report["error"] = getattr(error, "code", type(error).__name__)
        report["preparation"] = getattr(error, "receipt", getattr(error, "facts", {}))
        owner = getattr(error, "owner", None)
        if owner is not None:
            partial_owners.append(owner)
            t.worker_support.WINDOWS_UNCONFIRMED_OWNERS[id(owner)] = owner
    finally:
        if probe is not None:
            try:
                probe.close(cleanup_end=hard_end)
            except BaseException as error:
                report["cleanup_errors"].append(getattr(error, "code", "windows_probe_close"))
            report["fixture"]["probe_handles_closed"] = probe.closed
        if guard is not None:
            safe = (probe is None or probe.closed) and all(getattr(owner, "closed", False) for owner in partial_owners)
            for node in reversed(paths):
                entry = node["entry"]
                if safe and node["identity_confirmed"]:
                    try:
                        t.remaining(hard_end, time.monotonic)
                        current = guard.api.identify(entry["handle"]) if node["reparse"] else guard.api.inspect(entry["handle"])
                        t.require(current["signature"] == entry["facts"]["signature"], "windows_fixture_identity")
                        guard.close_entry(entry)
                        t.require(guard.api.remove_directory(node["path"]), "windows_fixture_remove")
                        node["removed"] = True
                    except BaseException as error:
                        safe = False
                        report["cleanup_errors"].append(getattr(error, "code", "windows_fixture_close"))
                else:
                    safe = False
            try:
                guard.close(remove_cwd=safe and all(node["removed"] for node in paths), cleanup_end=hard_end)
            except BaseException as error:
                report["cleanup_errors"].append(getattr(error, "code", "windows_fixture_close"))
            report["fixture"].update(root=guard.receipt(), root_handles_closed=guard.closed,
                                     root_closed=guard.created_directory is None,
                                     child_closed=bool(paths) and paths[-1]["removed"],
                                     unconfirmed_paths=[{"path": str(node["path"]), "creation_confirmed": node["creation_confirmed"],
                                                         "identity_confirmed": node["identity_confirmed"],
                                                         "handle": node["entry"]["handle"] if node["entry"] is not None else None,
                                                         "handle_closed": node["entry"]["closed"] if node["entry"] is not None else False}
                                                        for node in paths if not node["removed"]])
            if guard.created_directory is not None and not paths:
                report["fixture"]["unconfirmed_paths"].append({"path": str(guard.created_directory),
                                                               "creation_confirmed": False, "identity_confirmed": False})
            if not guard.closed or guard.created_directory is not None:
                t.worker_support.WINDOWS_UNCONFIRMED_OWNERS[id(guard)] = guard
                if report["error"] is None:
                    report["error"] = "windows_cleanup_unconfirmed"
        if plan is not None:
            report["runtime"].update(trust_handles_closed=plan["windows_guard"].closed,
                                     cwd_closed=plan["windows_guard"].created_directory is None)
            if not plan["windows_guard"].closed or plan["windows_guard"].created_directory is not None:
                t.worker_support.WINDOWS_UNCONFIRMED_OWNERS[id(plan["windows_guard"])] = plan["windows_guard"]
    if report["error"] is None and report["cleanup_errors"]:
        report["error"] = "windows_cleanup_unconfirmed"
    if time.monotonic() >= hard_end and report["error"] is None:
        report["error"] = "deadline"
    report["oracle"] = "satisfied" if report["error"] is None else \
        "not_run" if report["native_control"] == "NOT RUN" else "failed"
    report["elapsed_seconds"] = time.monotonic() - started
    return report



def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--case", choices=CASES)
    mode.add_argument("--trust-case", choices=TRUST_CASES)
    args = parser.parse_args()
    receipt = run_case(args.case) if args.case else run_trust_case(args.trust_case)
    print(json.dumps(receipt, sort_keys=True))
    return 0 if receipt["oracle"] == "satisfied" else 1


if __name__ == "__main__":
    raise SystemExit(main())
