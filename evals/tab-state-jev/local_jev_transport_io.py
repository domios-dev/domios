#!/usr/bin/env python3
"""WHAT: actual production helper against one owned loopback TLS listener.
WHY: pure mechanism controls cannot establish native exit/stream/handle closure.
ORACLE: exact request/one response and owned completion receipts for every case.
Public fixture EC key d=1 is intentionally public; no real token/config is read.
"""

import argparse
import base64
import hashlib
import json
from pathlib import Path
import socket
import ssl
import sys
import tempfile
import threading
import time

import jev_transport as t
import request_contract as c

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
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.bind(("127.0.0.1", 0))
        self.port = self.listener.getsockname()[1]
        self.listener.listen(1)
        self.ready, self.headers_sent, self.release, self.done = (threading.Event() for _ in range(4))
        self.thread = threading.Thread(target=self.serve, name="jev-fixture-server", daemon=False)
        self.requests, self.responses, self.request_bytes = 0, 0, None
        self.error, self.accepted_closed, self.listener_closed = None, False, False
        self.accepted = None
        self.lock = threading.Lock()

    def start(self):
        self.thread.start()
        event_wait(self.ready, self.operation_end, "fixture_not_ready")

    def serve(self):
        stream = None
        try:
            self.ready.set()
            self.listener.settimeout(t.remaining(self.operation_end, time.monotonic))
            plain, _ = self.listener.accept()
            with self.lock:
                self.accepted = plain
            plain.settimeout(t.remaining(self.operation_end, time.monotonic))
            stream = self.context.wrap_socket(plain, server_side=True)
            with self.lock:
                self.accepted = stream
            self.request_bytes = read_request(stream, self.operation_end)
            self.requests += 1
            body = c.encode({"answers": {"q": {"type": "choice", "choice": "opaque",
                                               "probabilities": {"opaque": 0.95}}}})
            status, extra = b"200 OK", b""
            if self.case == "response_oversize":
                body += b" " * 65537
            elif self.case == "redirect_refused":
                status, extra, body = b"302 Found", b"Location: https://elsewhere.invalid/\r\n", b""
            headers = b"HTTP/1.1 " + status + b"\r\n" + extra + b"Content-Length: " + str(len(body)).encode() + \
                b"\r\nConnection: close\r\n\r\n"
            stream.settimeout(t.remaining(self.operation_end, time.monotonic))
            stream.sendall(headers)
            self.headers_sent.set()
            self.responses += 1
            if self.case in ("held_response_deadline", "cancel_after_headers"):
                event_wait(self.release, self.hard_end, "fixture_hold_deadline")
            else:
                for offset in range(0, len(body), 4096):
                    stream.settimeout(t.remaining(self.operation_end, time.monotonic))
                    stream.sendall(body[offset:offset + 4096])
        except BaseException as error:
            if not (self.case in ("response_oversize", "redirect_refused") and isinstance(error, OSError)):
                self.error = getattr(error, "code", type(error).__name__)
        finally:
            try:
                if stream is not None:
                    stream.close()
                elif self.accepted is not None:
                    self.accepted.close()
                self.accepted_closed = self.accepted is not None
            except OSError:
                self.error = "fixture_peer_close"
            self.done.set()

    def close(self):
        self.release.set()
        with self.lock:
            if self.accepted is not None and not self.done.is_set():
                try:
                    self.accepted.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
        self.listener.close()
        self.listener_closed = True
        event_wait(self.done, self.hard_end, "fixture_server_unconfirmed")
        self.thread.join(t.remaining(self.hard_end, time.monotonic))
        if self.thread.is_alive():
            raise t.TransportFailure("fixture_thread_unconfirmed")
        return {"requests": self.requests, "responses": self.responses, "accepted_closed": self.accepted_closed,
                "listener_closed": self.listener_closed, "thread_joined": True, "error": self.error}


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


def fixture_plan_factory(ca_path, fixture):
    def build(wire, operation_end, hard_end, clock):
        plan = t.prepare_plan(wire, operation_end, hard_end, clock)
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
        plan["argv"] = plan["argv"][:4] + ["-c", bootstrap]
        return plan
    return build


def run_case(case):
    started = time.monotonic()
    run_end = started + 600
    hard_end = min(started + 60, run_end)
    operation_end = hard_end - 5
    t.native_support()
    wire = c.encode({"state": "synthetic loopback mechanism fixture", "model": "fixture-alias", "questions": {
        "q": {"type": "choice", "instructions": "opaque", "criteria": {"opaque": "opaque"}}}})
    fixture, cancel_thread, receipt = None, None, None
    outcome, fixture_receipt = None, None
    with tempfile.TemporaryDirectory(prefix="jev-public-tls-fixture-") as directory:
        root = Path(directory)
        certificate, key = public_fixture_certificate()
        ca_path, key_path = root / "public-test-ca.pem", root / "public-test-key.pem"
        ca_path.write_bytes(certificate)
        key_path.write_bytes(key)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(str(ca_path), str(key_path))
        fixture = LoopbackFixture(case, context, operation_end, hard_end)
        fixture.start()
        def lease_factory(plan, operation_end, hard_end, **kwargs):
            source = (lambda name, stream: PartialCapture(stream) if name == "stdout" else stream) \
                if case == "partial_capture_failure" else None
            return t.OwnedLease(plan, operation_end, hard_end, capture_source=source, **kwargs)
        transport = t.JevTransport(plan_factory=fixture_plan_factory(ca_path, fixture), lease_factory=lease_factory)
        cancel_error = []
        if case == "cancel_after_headers":
            def cancel_on_headers():
                try:
                    event_wait(fixture.headers_sent, operation_end, "fixture_headers_unconfirmed")
                    transport.cancel()
                except BaseException as error:
                    cancel_error.append(getattr(error, "code", type(error).__name__))
            cancel_thread = threading.Thread(target=cancel_on_headers, name="jev-fixture-cancel", daemon=False)
            cancel_thread.start()
        try:
            result = transport.send(wire, token=SYNTHETIC_TOKEN, timeout=hard_end - time.monotonic())
            receipt, outcome = result.receipt, "completed"
            if case != "success" or result.payload["answers"]["q"]["choice"] != "opaque":
                raise t.TransportFailure("fixture_unexpected_success", receipt)
        except t.TransportFailure as error:
            receipt, outcome = error.receipt, error.code
        finally:
            fixture_receipt = fixture.close()
            if cancel_thread is not None:
                cancel_thread.join(t.remaining(hard_end, time.monotonic))
                if cancel_thread.is_alive():
                    raise t.TransportFailure("fixture_cancel_thread_unconfirmed")
        expected = {"success": {"completed"}, "response_oversize": {"response_oversize"},
                    "redirect_refused": {"redirect_refused"}, "held_response_deadline": {"deadline"},
                    "cancel_after_headers": {"cancelled"}, "partial_capture_failure": {"cleanup_unconfirmed"}}[case]
        if outcome not in expected:
            raise t.TransportFailure("fixture_wrong_predicate", receipt)
        if receipt is None or not all(receipt.get(key) is True for key in (
                "created", "exit_observed", "reap_confirmed", "stdout_closed", "stderr_closed",
                "credential_channel_closed", "threads_joined", "handle_closed", "cwd_closed")):
            raise t.TransportFailure("fixture_native_completion_unconfirmed", receipt)
        if not all(fixture_receipt[key] for key in ("accepted_closed", "listener_closed", "thread_joined")) or \
                fixture_receipt["error"] is not None or cancel_error:
            raise t.TransportFailure("fixture_resource_unconfirmed", receipt)
        if fixture.request_bytes != wire or fixture.requests != 1 or fixture.responses != 1:
            raise t.TransportFailure("fixture_wire_or_count", receipt)
        if case == "partial_capture_failure":
            if receipt["completion_confirmed"] or not transport.poisoned or receipt["stdout_bytes"] <= 0:
                raise t.TransportFailure("fixture_partial_capture_laundered", receipt)
            try:
                transport.send(wire, token=SYNTHETIC_TOKEN, timeout=hard_end - time.monotonic())
            except t.TransportFailure as error:
                if error.code != "lease_unavailable":
                    raise
            else:
                raise t.TransportFailure("fixture_unknown_completion_admitted_request", receipt)
        elif receipt["completion_confirmed"] is not True:
            raise t.TransportFailure("fixture_capture_completion_unconfirmed", receipt)
        return {"case": case, "oracle": "satisfied", "platform": sys.platform,
                "interpreter": sys.version.split()[0], "fixture_type": "synthetic_mechanism",
                "outcome": outcome, "expected_requests": 1, "expected_responses": 1,
                "python_wire_bytes_sha256": c.sha256(wire), "public_test_ca_bytes_sha256": c.sha256(certificate),
                "transport": receipt, "fixture": fixture_receipt}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", choices=CASES, required=True)
    args = parser.parse_args()
    try:
        receipt = run_case(args.case)
    except BaseException as error:
        print(json.dumps({"case": args.case, "oracle": "failed", "platform": sys.platform,
                          "error": getattr(error, "code", type(error).__name__),
                          "transport": getattr(error, "receipt", {})}, sort_keys=True))
        return 1
    print(json.dumps(receipt, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
