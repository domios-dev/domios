"""Trusted one-request interpreter helper; no subprocesses or descendant spawning."""

import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import socket
import ssl
import stat
import struct
import sys
import time
from urllib.parse import urlsplit

ENDPOINT = "https://api.typesafe.ai/v1/systemone"
CHUNK = 4096
MAX_REQUEST = 256 * 1024
MAX_RESPONSE = 64 * 1024
MAX_HEADERS = 16 * 1024
MAX_FIELDS = 100
MAX_DIAGNOSTIC = 4096
MAX_TOKEN = 4096
MAX_STDOUT = 4096


class WorkerError(ValueError):
    def __init__(self, code, **facts):
        self.code, self.facts = code, facts
        super().__init__(code)


def require(value, code, **facts):
    if not value:
        raise WorkerError(code, **facts)


def remaining(end, clock=time.monotonic):
    require(type(end) in (int, float) and math.isfinite(end), "deadline_schema")
    value = end - clock()
    require(value > 0, "deadline")
    return value


def read_bounded_response(response, *, limit=MAX_RESPONSE, end, clock=time.monotonic):
    chunks, count = [], 0
    while True:
        remaining(end, clock)
        bound = min(CHUNK, limit + 1 - count)
        chunk = response.read(bound)
        remaining(end, clock)
        require(type(chunk) is bytes and len(chunk) <= bound, "response_read_shape", response_bytes=count)
        if not chunk:
            return b"".join(chunks)
        count += len(chunk)
        require(count <= limit, "response_oversize" if limit == MAX_RESPONSE else "error_body_oversize",
                response_bytes=count, response_limit=limit)
        chunks.append(chunk)


class SocketReader:
    def __init__(self, stream, end, clock):
        self.stream, self.end, self.clock = stream, end, clock
        self.buffer = b""
        self.header_bytes = 0
        self.fields = 0

    def _recv(self, size):
        self.stream.settimeout(remaining(self.end, self.clock))
        raw = self.stream.recv(min(CHUNK, size))
        remaining(self.end, self.clock)
        return raw

    def read(self, size):
        require(type(size) is int and 0 < size <= CHUNK, "read_bound")
        if self.buffer:
            raw, self.buffer = self.buffer[:size], self.buffer[size:]
            return raw
        return self._recv(size)

    def line(self, *, field=False, line_limit=MAX_HEADERS):
        limit = min(line_limit, MAX_HEADERS - self.header_bytes)
        require(limit >= 2, "header_oversize", header_bytes=self.header_bytes)
        while b"\n" not in self.buffer:
            require(len(self.buffer) < limit, "header_oversize", header_bytes=self.header_bytes + len(self.buffer))
            raw = self._recv(min(CHUNK, limit - len(self.buffer)))
            require(bool(raw), "header_incomplete")
            self.buffer += raw
        index = self.buffer.index(b"\n") + 1
        require(index <= limit and self.buffer[index - 2:index] == b"\r\n", "header_shape")
        raw, self.buffer = self.buffer[:index], self.buffer[index:]
        self.header_bytes += len(raw)
        if field and raw != b"\r\n":
            self.fields += 1
            require(self.fields <= MAX_FIELDS, "header_fields", header_fields=self.fields)
        return raw[:-2]

    def headers(self):
        headers = {}
        while True:
            raw = self.line(field=True)
            if not raw:
                return headers
            require(b":" in raw and not raw.startswith((b" ", b"\t")), "header_shape")
            name, value = raw.split(b":", 1)
            require(re.fullmatch(br"[!#$%&'*+.^_`|~0-9A-Za-z-]+", name) is not None, "header_name")
            require(all(byte == 9 or 32 <= byte <= 126 for byte in value), "header_value")
            key = name.decode("ascii").lower()
            require(key not in headers, "duplicate_header")
            headers[key] = value.strip().decode("ascii")


class HttpBody:
    def __init__(self, reader, status, headers):
        self.reader, self.status, self.headers = reader, status, headers
        require("content-encoding" not in headers, "content_encoding")
        require(not ("content-length" in headers and "transfer-encoding" in headers), "ambiguous_body")
        length = headers.get("content-length")
        require(length is None or re.fullmatch(r"[0-9]{1,10}", length), "content_length")
        self.left = int(length) if length is not None else None
        encoding = headers.get("transfer-encoding")
        require(encoding in (None, "chunked"), "transfer_encoding")
        self.chunked, self.chunk_left, self.finished = encoding == "chunked", 0, False
        limit = MAX_RESPONSE if status == 200 else MAX_DIAGNOSTIC
        require(self.left is None or self.left <= limit, "response_oversize" if status == 200 else "error_body_oversize",
                response_limit=limit, declared_response_bytes=self.left, response_bytes=0)

    def read(self, size):
        require(type(size) is int and 0 < size <= CHUNK, "read_bound")
        if self.finished:
            return b""
        if self.chunked:
            if self.chunk_left == 0:
                line = self.reader.line(line_limit=128)
                require(re.fullmatch(br"[0-9a-fA-F]{1,8}", line), "chunk_shape")
                self.chunk_left = int(line, 16)
                if self.chunk_left == 0:
                    self.reader.headers()
                    self.finished = True
                    return b""
            raw = self.reader.read(min(size, self.chunk_left))
            require(bool(raw), "body_incomplete")
            self.chunk_left -= len(raw)
            if self.chunk_left == 0:
                require(self.reader.line(line_limit=2) == b"", "chunk_terminator")
            return raw
        if self.left == 0:
            self.finished = True
            return b""
        raw = self.reader.read(min(size, self.left) if self.left is not None else size)
        if self.left is not None:
            require(bool(raw), "body_incomplete")
            self.left -= len(raw)
        else:
            self.finished = not raw
        return raw


def trust_context():
    context = ssl.create_default_context()
    source = "OS/default-public-roots"
    if sys.platform == "darwin" and not context.get_ca_certs(binary_form=True):
        path = Path("/etc/ssl/cert.pem").resolve(strict=True)
        identity = path.stat()
        require(stat.S_ISREG(identity.st_mode) and identity.st_uid == 0 and
                not identity.st_mode & 0o022 and 0 < identity.st_size <= 4 * 1024 * 1024, "trust_root_source")
        context = ssl.create_default_context(cafile=str(path))
        source = "darwin-root-owned:/etc/ssl/cert.pem"
    return context, source


def trust_receipt(context, source):
    require(context.verify_mode == ssl.CERT_REQUIRED and context.check_hostname is True and
            context.keylog_filename is None, "tls_verification")
    roots = sorted(context.get_ca_certs(binary_form=True))
    require(bool(roots) and len(roots) <= 4096, "trust_roots_absent")
    raw = b"public-trust-root-der-set-v1\0" + b"".join(struct.pack("!I", len(root)) + root for root in roots)
    require(len(raw) <= 4 * 1024 * 1024, "trust_roots_size")
    return {"root_provenance": source, "root_der_set_sha256": hashlib.sha256(raw).hexdigest(),
            "root_count": len(roots), "certificate_verified": True, "hostname_verified": True}


def endpoint_policy(endpoint, policy=None):
    require(endpoint == ENDPOINT if policy is None else endpoint == policy["endpoint"], "endpoint_policy")
    parsed = urlsplit(endpoint)
    require(parsed.scheme == "https" and not parsed.username and not parsed.password and
            not parsed.query and not parsed.fragment, "endpoint_policy")
    if policy is not None:
        require(parsed.hostname == "127.0.0.1" and parsed.port == policy["port"], "local_listener_policy")
    return parsed


def https_exchange(raw, token, end, *, policy=None, clock=time.monotonic):
    endpoint = ENDPOINT if policy is None else policy["endpoint"]
    parsed = endpoint_policy(endpoint, policy)
    context, source = trust_context() if policy is None else (policy["context"], "fixture-local-public-test-CA")
    tls = trust_receipt(context, source)
    plain, encrypted, reader = None, None, None
    facts = {"tls": tls, "network_attempted": False, "socket_closed": False}
    try:
        remaining(end, clock)
        facts["network_attempted"] = True
        plain = socket.create_connection((parsed.hostname, parsed.port or 443), timeout=remaining(end, clock))
        remaining(end, clock)
        plain.settimeout(remaining(end, clock))
        encrypted = context.wrap_socket(plain, server_hostname=parsed.hostname)
        plain = None
        remaining(end, clock)
        authorization = b"Bearer " + token
        header = (b"POST " + parsed.path.encode("ascii") + b" HTTP/1.1\r\nHost: " +
                  parsed.netloc.encode("ascii") + b"\r\nAuthorization: " + authorization +
                  b"\r\nContent-Type: application/json\r\nAccept-Encoding: identity\r\nConnection: close\r\nContent-Length: " +
                  str(len(raw)).encode("ascii") + b"\r\n\r\n")
        require(len(header) <= MAX_HEADERS, "request_header_size")
        for data in (header, raw):
            offset = 0
            while offset < len(data):
                encrypted.settimeout(remaining(end, clock))
                sent = encrypted.send(data[offset:offset + CHUNK])
                require(sent > 0, "request_write")
                offset += sent
                remaining(end, clock)
        reader = SocketReader(encrypted, end, clock)
        status_line = reader.line()
        require(re.fullmatch(br"HTTP/1\.[01] [1-5][0-9]{2}( [\x20-\x7e]*)?", status_line), "http_status_shape")
        status = int(status_line.split(b" ")[1])
        headers = reader.headers()
        facts.update(http_status=status, header_bytes=reader.header_bytes, header_fields=reader.fields)
        require(not 300 <= status < 400, "redirect_refused", http_status=status)
        response = HttpBody(reader, status, headers)
        body = read_bounded_response(response, limit=MAX_RESPONSE if status == 200 else MAX_DIAGNOSTIC,
                                     end=end, clock=clock)
        facts["response_bytes"] = len(body)
        require(status == 200, "http_status", http_status=status, response_bytes=len(body))
        return body, facts
    except BaseException as error:
        if reader is not None:
            facts.update(header_bytes=reader.header_bytes, header_fields=reader.fields)
        if isinstance(error, WorkerError):
            error.facts = {**facts, **error.facts}
            facts = error.facts
            raise
        code = "transport_tls" if isinstance(error, ssl.SSLError) else \
            "deadline" if isinstance(error, TimeoutError) else "transport_os"
        pending = WorkerError(code, **facts)
        facts = pending.facts
        raise pending from error
    finally:
        try:
            if encrypted is not None:
                encrypted.close()
            if plain is not None:
                plain.close()
            facts["socket_closed"] = True
        except BaseException as error:
            raise WorkerError("socket_close_unconfirmed", **facts) from error


def file_hash(path, end):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        while True:
            remaining(end)
            raw = stream.read(CHUNK)
            if not raw:
                break
            digest.update(raw)
    remaining(end)
    return digest.hexdigest()


def frame(stream, limit):
    prefix = stream.read(4)
    require(len(prefix) == 4, "channel_frame")
    length = struct.unpack("!I", prefix)[0]
    require(0 < length <= limit, "channel_size")
    chunks, count = [], 0
    while count < length:
        raw = stream.read(min(CHUNK, length - count))
        require(bool(raw), "channel_incomplete")
        count += len(raw)
        chunks.append(raw)
    return b"".join(chunks)


def load_contract(header, end):
    own = Path(__file__).resolve()
    contract = own.with_name("request_contract.py")
    require(file_hash(own, end) == header["worker_sha256"] and
            file_hash(contract, end) == header["contract_sha256"] and
            Path(sys.executable).resolve() == Path(header["interpreter"]) and
            file_hash(sys.executable, end) == header["interpreter_sha256"], "trusted_identity")
    spec = importlib.util.spec_from_file_location("request_contract", contract)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def helper_input():
    require(len(sys.argv) == 1 and sys.flags.isolated and sys.flags.no_site and
            sys.flags.dont_write_bytecode, "isolated_startup")
    require(not any(key.startswith(("PYTHON", "JEV", "TYPESAFE", "CODETERM")) or
                    key.lower().endswith("proxy") or key in ("SSL_CERT_FILE", "SSL_CERT_DIR", "SSLKEYLOGFILE")
                    for key in os.environ), "ambient_environment")
    raw_header = frame(sys.stdin.buffer, MAX_DIAGNOSTIC)
    header = json.loads(raw_header)
    require(type(header) is dict and set(header) == {"version", "operation_end", "hard_end", "endpoint",
            "wire_sha256", "worker_sha256", "contract_sha256", "interpreter", "interpreter_sha256", "cwd"}
            and header["version"] == 1, "parent_schema")
    end = header["operation_end"]
    remaining(end)
    require(type(header["hard_end"]) in (int, float) and math.isfinite(header["hard_end"]) and
            5 <= header["hard_end"] - end <= 5.001, "cleanup_reserve")
    endpoint_policy(header["endpoint"])
    cwd = Path(header["cwd"])
    require(cwd.is_absolute() and cwd.resolve() == cwd and Path.cwd() == cwd and not cwd.is_symlink(), "private_cwd")
    identity = cwd.stat()
    require(os.name == "posix" and identity.st_uid == os.getuid() and
            stat.S_IMODE(identity.st_mode) == 0o700, "private_cwd_permissions")
    contract = load_contract(header, end)
    wire = frame(sys.stdin.buffer, MAX_REQUEST)
    require(hashlib.sha256(wire).hexdigest() == header["wire_sha256"], "wire_hash")
    request = contract.validate_wire(wire)
    remaining(end)
    token = frame(sys.stdin.buffer, MAX_TOKEN)
    require(all(33 <= byte <= 126 for byte in token), "token_preflight")
    require(sys.stdin.buffer.read(1) == b"", "channel_trailing_bytes")
    sys.stdin.close()
    remaining(end)
    return header, wire, request, token, contract


def main(*, policy=None):
    receipt = {"version": 1, "credential_channel_closed": False, "network_attempted": False}
    code = 1
    try:
        header, wire, request, token, contract = helper_input()
        receipt["credential_channel_closed"] = True
        receipt["wire_sha256"] = header["wire_sha256"]
        body, facts = https_exchange(wire, token, header["operation_end"], policy=policy)
        receipt.update(facts)
        answers = contract.validate_answers(contract.decode(body, MAX_RESPONSE), request["questions"])
        receipt.update(status="completed", answers=answers, response_bytes_sha256=hashlib.sha256(body).hexdigest())
        remaining(header["operation_end"])
        code = 0
    except BaseException as error:
        if isinstance(error, WorkerError):
            receipt.update(error.facts)
            reason = error.code
        elif isinstance(error, ssl.SSLError):
            reason = "transport_tls"
        elif isinstance(error, (TimeoutError, socket.timeout)):
            reason = "deadline"
        elif isinstance(error, OSError):
            reason = "transport_os"
        else:
            reason = getattr(error, "code", "helper_schema")
        receipt.update(status="failed", error=reason)
        receipt.pop("answers", None)
    finally:
        try:
            sys.stdin.close()
            receipt["credential_channel_closed"] = True
        except OSError:
            receipt.update(status="failed", error="channel_close")
            code = 1
    raw = json.dumps(receipt, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    if len(raw) > MAX_STDOUT:
        compact = {key: receipt[key] for key in ("credential_channel_closed", "network_attempted", "wire_sha256",
                   "socket_closed", "http_status", "header_bytes", "header_fields", "response_bytes") if key in receipt}
        compact.update(version=1, status="failed", error="capture_size", receipt_bytes=len(raw), receipt_limit=MAX_STDOUT)
        raw = json.dumps(compact, sort_keys=True, separators=(",", ":")).encode()
        code = 1
    try:
        require(sys.stdout.buffer.write(raw) == len(raw), "receipt_write")
        sys.stdout.close()
        sys.stderr.close()
    except OSError:
        return 1
    return code


if __name__ == "__main__":
    sys.exit(main())
