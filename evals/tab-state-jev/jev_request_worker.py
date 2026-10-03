"""Trusted one-request interpreter helper; no subprocesses or descendant spawning."""

from contextlib import contextmanager
import ctypes
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import secrets
import socket
import ssl
import stat
import struct
import sys
import sysconfig
import tempfile
import types
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
WINDOWS_ACTIVE_TRUST = None
WINDOWS_UNCONFIRMED_OWNERS = {}


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


WINDOWS_SUBPROCESS_SHA256 = "a7b6bdf4657b018e4e76f22ebbb975a5fa6f0501088177fe950df1e20c18b209"
WINDOWS_ADMINS = frozenset(("S-1-5-18", "S-1-5-32-544"))
WINDOWS_SERVICING_OWNER = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"
WINDOWS_MUTATION = 0x500D0156
WINDOWS_REPLACEMENT = 0x500D0140


def windows_acl_policy(security, user, *, private=False, ancestor=False, volume_root=False):
    trusted = WINDOWS_ADMINS | {user}
    servicing_root = volume_root and ancestor and not private and security["owner"] == WINDOWS_SERVICING_OWNER
    require(security["owner"] in trusted or servicing_root, "windows_owner")
    require(security["dacl_present"] and security["aces"] is not None, "windows_dacl")
    require(not private or security["owner"] == user and security["protected"], "windows_private_dacl")
    for kind, flags, mask, sid in security["aces"]:
        require(kind in (0, 1) and not flags & ~0x1f, "windows_ace_unsupported")
        if kind == 0 and sid not in trusted:
            if ancestor and flags & 0x8:
                continue
            forbidden = mask if private else mask & (WINDOWS_REPLACEMENT if ancestor else WINDOWS_MUTATION)
            require(not forbidden, "windows_foreign_authority")


class WindowsApi:
    def __init__(self):
        self.allocations = set()
        self.owners = []
        self.token_closed = False
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.security = ctypes.WinDLL("advapi32", use_last_error=True)
        p, u, b, s = ctypes.c_void_p, ctypes.c_uint32, ctypes.c_int32, ctypes.c_wchar_p
        pp = ctypes.POINTER(p)
        bindings = ((self.kernel, "CreateFileW", (s, u, u, p, u, u, p), p),
                    (self.kernel, "CloseHandle", (p,), b), (self.kernel, "LocalFree", (p,), p),
                    (self.kernel, "GetCurrentProcess", (), p),
                    (self.kernel, "GetFileType", (p,), u), (self.kernel, "GetDriveTypeW", (s,), u),
                    (self.kernel, "GetFileInformationByHandleEx", (p, u, p, u), b),
                    (self.kernel, "GetFinalPathNameByHandleW", (p, p, u, u), u),
                    (self.kernel, "GetVolumeInformationByHandleW", (p, p, u, p, p, p, p, u), b),
                    (self.kernel, "ReadFile", (p, p, u, p, p), b),
                    (self.kernel, "CreateDirectoryW", (s, p), b), (self.kernel, "RemoveDirectoryW", (s,), b),
                    (self.security, "OpenProcessToken", (p, u, pp), b),
                    (self.security, "GetTokenInformation", (p, u, p, u, p), b),
                    (self.security, "GetSecurityInfo", (p, u, u, pp, pp, pp, pp, pp), u),
                    (self.security, "GetSecurityDescriptorLength", (p,), u),
                    (self.security, "GetSecurityDescriptorControl", (p, p, p), b),
                    (self.security, "GetSecurityDescriptorDacl", (p, p, pp, p), b),
                    (self.security, "GetAce", (p, u, pp), b), (self.security, "IsValidAcl", (p,), b),
                    (self.security, "IsValidSid", (p,), b), (self.security, "GetLengthSid", (p,), u),
                    (self.security, "ConvertSidToStringSidW", (p, pp), b),
                    (self.security, "ConvertStringSecurityDescriptorToSecurityDescriptorW", (s, u, pp, p), b))
        for dll, name, arguments, result in bindings:
            function = getattr(dll, name)
            function.argtypes, function.restype = arguments, result

    def captured(self, kind, native):
        identity = native.value if isinstance(native, ctypes.c_void_p) else native
        entry = {"kind": kind, "identity": identity, "native": native, "release_attempted": False, "closed": False}
        self.owners.append(entry)
        return entry

    def record(self, kind, identity):
        return next((entry for entry in reversed(self.owners)
                     if entry["kind"] == kind and entry["identity"] == identity), None)

    def release(self, entry):
        if entry is None or entry["closed"]:
            return
        require(not entry["release_attempted"], "windows_release_unconfirmed")
        entry["release_attempted"] = True
        try:
            if entry["kind"] == "allocation":
                require(not self.kernel.LocalFree(entry["native"]), "windows_descriptor_close")
                self.allocations.discard(entry["identity"])
            else:
                require(self.kernel.CloseHandle(entry["native"]), "windows_handle_close")
            entry["closed"] = True
            if entry["kind"] == "token":
                self.token_closed = True
            if entry["kind"] == "allocation":
                self.owners.remove(entry)
        except BaseException as error:
            entry["error"] = getattr(error, "code", "windows_release_os")
            WINDOWS_UNCONFIRMED_OWNERS[id(self)] = self
            raise

    def failed(self, error):
        failure = error if isinstance(error, WorkerError) else WorkerError("windows_native_os")
        failure.owner = self
        failure.facts["native_resources"] = self.receipt()
        if not self.settled():
            WINDOWS_UNCONFIRMED_OWNERS[id(self)] = self
        return failure

    @contextmanager
    def retiring(self, entry):
        failure = None
        try:
            yield
        except BaseException as error:
            failure = error if isinstance(error, WorkerError) else WorkerError("windows_native_os")
        try:
            self.release(entry)
        except BaseException as error:
            if failure is None:
                failure = error
            else:
                failure.facts.setdefault("cleanup_errors", []).append(getattr(error, "code", "windows_release_os"))
        if failure is not None:
            raise self.failed(failure)

    def admit_native(self):
        require(not any(entry["release_attempted"] and not entry["closed"] for entry in self.owners),
                "windows_native_resource_unconfirmed")

    def settled(self):
        return all(entry["closed"] for entry in self.owners) and not self.allocations

    def settle_known(self):
        first = None
        for entry in tuple(reversed(self.owners)):
            if entry["closed"] or entry["release_attempted"]:
                continue
            try:
                self.release(entry)
            except BaseException as error:
                first = first or error
        if first is not None:
            raise self.failed(first)
        require(self.settled(), "windows_native_resource_unconfirmed")

    def close(self, handle):
        identity = handle.value if isinstance(handle, ctypes.c_void_p) else handle
        entry = self.record("file", identity) or self.record("token", identity)
        require(entry is not None, "windows_unknown_handle")
        self.release(entry)

    def free(self, pointer):
        entry = self.record("allocation", pointer.value)
        require(entry is not None, "windows_unknown_allocation")
        self.release(entry)

    def allocated(self, pointer):
        if pointer.value:
            self.allocations.add(pointer.value)
            return self.captured("allocation", pointer)
        return None

    def receipt(self):
        resources = [entry for entry in self.owners if entry["kind"] == "token" or not entry["closed"]]
        return {"token_handle_closed": self.token_closed, "unclosed_native_allocations": len(self.allocations),
                "owned_resources": [{key: entry[key] for key in ("kind", "identity", "release_attempted", "closed")}
                                    for entry in resources[:16]], "owned_resources_count": len(resources),
                "resources_omitted": max(0, len(resources) - 16), "all_resources_closed": self.settled()}

    def sid(self, pointer):
        require(pointer and self.security.IsValidSid(pointer) and self.security.GetLengthSid(pointer) <= 68, "windows_sid")
        self.admit_native()
        raw = ctypes.c_void_p()
        result = self.security.ConvertSidToStringSidW(pointer, ctypes.byref(raw))
        entry = self.allocated(raw)
        with self.retiring(entry):
            require(result and raw.value, "windows_sid")
            value = ctypes.wstring_at(raw)
            require(len(value) <= 184, "windows_sid")
            return value

    def current_user(self):
        self.admit_native()
        token = ctypes.c_void_p()
        result = self.security.OpenProcessToken(self.kernel.GetCurrentProcess(), 8, ctypes.byref(token))
        require(result and token.value, "windows_token")
        entry = self.captured("token", token)
        with self.retiring(entry):
            size = ctypes.c_uint32()
            self.security.GetTokenInformation(token, 1, None, 0, ctypes.byref(size))
            require(0 < size.value <= MAX_HEADERS, "windows_token_size")
            raw = ctypes.create_string_buffer(size.value)
            require(self.security.GetTokenInformation(token, 1, raw, size.value, ctypes.byref(size)), "windows_token")
            return self.sid(ctypes.c_void_p.from_buffer(raw).value)


    def descriptor(self, handle):
        self.admit_native()
        owner, acl, descriptor = (ctypes.c_void_p() for _ in range(3))
        result = self.security.GetSecurityInfo(handle, 1, 5, ctypes.byref(owner), None,
                                               ctypes.byref(acl), None, ctypes.byref(descriptor))
        entry = self.allocated(descriptor)
        with self.retiring(entry):
            require(result == 0 and descriptor.value, "windows_security_read")
            length = self.security.GetSecurityDescriptorLength(descriptor)
            require(0 < length <= MAX_HEADERS, "windows_security_size")
            control, revision = ctypes.c_uint16(), ctypes.c_uint32()
            require(self.security.GetSecurityDescriptorControl(descriptor, ctypes.byref(control), ctypes.byref(revision)),
                    "windows_security_control")
            require(control.value & 0x8000 and revision.value == 1, "windows_security_form")
            present, defaulted = ctypes.c_int32(), ctypes.c_int32()
            require(self.security.GetSecurityDescriptorDacl(descriptor, ctypes.byref(present), ctypes.byref(acl),
                                                           ctypes.byref(defaulted)), "windows_dacl")
            records = None
            if acl.value:
                require(self.security.IsValidAcl(acl), "windows_dacl")
                header = ctypes.string_at(acl, 8)
                acl_revision, _, acl_size, count, _ = struct.unpack("<BBHHH", header)
                require(acl_revision in (2, 4) and 8 <= acl_size <= MAX_HEADERS and count <= MAX_FIELDS, "windows_acl_size")
                records = []
                for index in range(count):
                    ace = ctypes.c_void_p()
                    require(self.security.GetAce(acl, index, ctypes.byref(ace)), "windows_ace")
                    require(acl.value <= ace.value and ace.value + 8 <= acl.value + acl_size, "windows_ace_size")
                    kind, flags, size, mask = struct.unpack("<BBHI", ctypes.string_at(ace, 8))
                    require(kind in (0, 1) and 16 <= size <= acl_size and
                            acl.value <= ace.value and ace.value + size <= acl.value + acl_size, "windows_ace_unsupported")
                    sid = ace.value + 8
                    require(self.security.IsValidSid(sid) and 8 + self.security.GetLengthSid(sid) == size, "windows_ace_sid")
                    records.append((kind, flags, mask, self.sid(sid)))
            raw = ctypes.string_at(descriptor, length)
            return {"owner": self.sid(owner), "dacl_present": bool(present.value), "aces": records,
                    "protected": bool(control.value & 0x1000), "bytes": raw}

    def identify(self, handle):
        self.admit_native()
        require(self.kernel.GetFileType(handle) == 1, "windows_object_type")
        basic, standard, identity = (ctypes.create_string_buffer(size) for size in (40, 24, 24))
        for kind, raw in ((0, basic), (1, standard), (18, identity)):
            require(self.kernel.GetFileInformationByHandleEx(handle, kind, raw, len(raw)), "windows_file_identity")
        creation, _, written, changed, attributes = struct.unpack_from("<qqqqI", basic.raw)
        _, size, links, deleting, directory = struct.unpack_from("<qqIBB", standard.raw)
        require(not deleting, "windows_object_deleting")
        filesystem = ctypes.create_unicode_buffer(32)
        flags = ctypes.c_uint32()
        require(self.kernel.GetVolumeInformationByHandleW(handle, None, 0, None, None, ctypes.byref(flags),
                                                         filesystem, len(filesystem)) and
                filesystem.value == "NTFS" and flags.value & 8, "windows_local_ntfs")
        final = ctypes.create_unicode_buffer(32768)
        length = self.kernel.GetFinalPathNameByHandleW(handle, final, len(final), 0)
        require(0 < length < len(final) and final.value.startswith("\\\\?\\") and
                re.fullmatch(r"[A-Za-z]:\\.*", final.value[4:]), "windows_local_path")
        require(self.kernel.GetDriveTypeW(final.value[4:7]) == 3, "windows_local_volume")
        security = self.descriptor(handle)
        return {"identity": identity.raw, "size": size, "directory": bool(directory), "security": security,
                "attributes": attributes, "links": links,
                "signature": (identity.raw, creation, attributes, security["bytes"]) if directory else
                (identity.raw, creation, written, changed, attributes, size, security["bytes"]),
                "final": final.value[4:]}

    def inspect(self, handle):
        facts = self.identify(handle)
        require(not facts["attributes"] & 0x400 and (facts["directory"] or facts["links"] == 1),
                "windows_reparse_or_links")
        return facts

    def open(self, path, *, directory=False):
        self.admit_native()
        access = 0x20080 if directory else 0x80020000
        handle = self.kernel.CreateFileW(str(path), access, 1, None, 3, 0x02200000, None)
        require(handle not in (None, 0, ctypes.c_void_p(-1).value), "windows_open")
        self.captured("file", handle)
        return handle

    def remove_directory(self, path):
        return bool(self.kernel.RemoveDirectoryW(str(path)))

    def read(self, handle):
        raw, size = ctypes.create_string_buffer(CHUNK), ctypes.c_uint32()
        require(self.kernel.ReadFile(handle, raw, CHUNK, ctypes.byref(size), None) and size.value <= CHUNK,
                "windows_read")
        return raw.raw[:size.value]

    def create_directory(self, path, sddl):
        self.admit_native()
        class Attributes(ctypes.Structure):
            _fields_ = [("length", ctypes.c_uint32), ("descriptor", ctypes.c_void_p), ("inherit", ctypes.c_int32)]
        descriptor = ctypes.c_void_p()
        result = self.security.ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl, 1, ctypes.byref(descriptor), None)
        entry = self.allocated(descriptor)
        with self.retiring(entry):
            require(result and descriptor.value, "windows_creation_descriptor")
            attributes = Attributes(ctypes.sizeof(Attributes), descriptor, 0)
            require(self.kernel.CreateDirectoryW(str(path), ctypes.byref(attributes)), "windows_directory_create")


class WindowsTrust:
    def __init__(self, end, *, clock=time.monotonic, api=None):
        self.end, self.clock = end, clock
        self.objects, self.cwd, self.api, self.user = [], None, None, None
        self.created_directory = None
        self.closed, self.lost = False, False
        try:
            self.check_time()
            self.api = WindowsApi() if api is None else api
            self.user = self.api.current_user()
        except BaseException as error:
            failure = error if isinstance(error, WorkerError) else WorkerError("windows_initialization_os")
            try:
                self.close()
            except BaseException as cleanup:
                failure.facts.setdefault("cleanup_errors", []).append(getattr(cleanup, "code", "windows_initialization_cleanup"))
            failure.owner = self
            failure.facts.update(windows_trust=self.receipt(), trust_handles_closed=self.closed,
                                 cwd_closed=self.created_directory is None, constructor_complete=False)
            if not self.closed:
                WINDOWS_UNCONFIRMED_OWNERS[id(self)] = self
            raise failure

    def check_time(self):
        remaining(self.end, self.clock)

    def capture(self, path, *, directory=False):
        self.check_time()
        path = Path(os.path.abspath(path))
        require(re.fullmatch(r"[A-Za-z]:[\\/].*", str(path)), "windows_local_path")
        handle = self.api.open(path, directory=directory)
        entry = {"path": path, "handle": handle, "closed": False}
        self.objects.append(entry)
        return entry

    def opened(self, path, *, directory=False, private=False, ancestor=False):
        entry = self.capture(path, directory=directory)
        facts = self.api.inspect(entry["handle"])
        entry["facts"] = facts
        require(facts["directory"] == directory and os.path.normcase(facts["final"]) == os.path.normcase(str(entry["path"])),
                "windows_path_identity")
        volume_root = directory and entry["path"].parent == entry["path"]
        windows_acl_policy(facts["security"], self.user, private=private, ancestor=ancestor, volume_root=volume_root)
        self.check_time()
        return entry

    def file(self, path, *, collect=False):
        path = Path(os.path.abspath(path))
        for parent in reversed(path.parents):
            self.opened(parent, directory=True, ancestor=True)
        entry = self.opened(path)
        digest, chunks, size = hashlib.sha256(), [], 0
        while True:
            self.check_time()
            raw = self.api.read(entry["handle"])
            require(type(raw) is bytes and len(raw) <= CHUNK, "windows_read_shape")
            if not raw:
                break
            digest.update(raw)
            if collect:
                size += len(raw)
                require(size <= MAX_REQUEST, "windows_source_size")
                chunks.append(raw)
        self.revalidate()
        return entry["path"], digest.hexdigest(), b"".join(chunks)

    def directory(self, path, *, private=False):
        path = Path(os.path.abspath(path))
        for parent in reversed(path.parents):
            self.opened(parent, directory=True, ancestor=True)
        entry = self.opened(path, directory=True, private=private)
        if private:
            self.cwd = entry
        return entry

    def revalidate(self, *, end=None):
        end = self.end if end is None else end
        require(self.clock() < end, "deadline")
        for entry in self.objects:
            if entry["closed"]:
                continue
            require(self.clock() < end, "deadline")
            facts = self.api.inspect(entry["handle"])
            if "facts" not in entry or facts["signature"] != entry["facts"]["signature"] or \
                    os.path.normcase(facts["final"]) != os.path.normcase(str(entry["path"])):
                self.lost = True
                raise WorkerError("windows_identity_changed")
        require(self.clock() < end, "deadline")

    def create_private(self):
        parent = Path(os.path.abspath(tempfile.gettempdir()))
        self.directory(parent)
        path = parent / ("jev-owned-" + secrets.token_hex(16))
        sddl = f"O:{self.user}D:P(A;OICI;FA;;;{self.user})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"
        self.created_directory = path
        try:
            self.api.create_directory(path, sddl)
            self.directory(path, private=True)
        except BaseException as error:
            failure = error if isinstance(error, WorkerError) else WorkerError("windows_private_creation_unconfirmed")
            failure.facts.update(directory_creation_unconfirmed=True, windows_trust=self.receipt())
            failure.owner = self
            WINDOWS_UNCONFIRMED_OWNERS[id(self)] = self
            raise failure
        return path

    def close_entry(self, entry):
        if entry["closed"]:
            return
        require(not entry.get("close_attempted"), "windows_close_unconfirmed")
        entry["close_attempted"] = True
        try:
            self.api.close(entry["handle"])
            entry["closed"] = True
        except BaseException:
            self.lost = True
            raise

    def close(self, *, remove_cwd=False, cleanup_end=None):
        if self.closed:
            return
        end = self.end if cleanup_end is None else cleanup_end
        first = None
        if remove_cwd:
            try:
                require(not self.lost and self.cwd is not None, "windows_cleanup_identity")
                self.revalidate(end=end)
                self.close_entry(self.cwd)
                require(self.api.remove_directory(self.cwd["path"]), "windows_directory_remove")
                self.created_directory = None
            except BaseException as error:
                first = error
        for entry in reversed(self.objects):
            try:
                self.close_entry(entry)
            except BaseException as error:
                first = first or error
        if self.api is not None and callable(getattr(self.api, "settle_known", None)):
            try:
                self.api.settle_known()
            except BaseException as error:
                first = first or error
        native_closed = self.api.settled() if callable(getattr(self.api, "settled", None)) else \
            not getattr(self.api, "allocations", ()) and getattr(self.api, "token_closed", True)
        self.closed = all(entry["closed"] for entry in self.objects) and native_closed
        if not self.closed:
            WINDOWS_UNCONFIRMED_OWNERS[id(self)] = self
        if first is not None:
            raise first
        require(self.closed, "windows_native_resource_unconfirmed")
        require(self.clock() < end, "deadline")

    def receipt(self):
        native = self.api.receipt() if callable(getattr(self.api, "receipt", None)) else {"fixture_type": "synthetic_mechanism"}
        return {"native_resources": native, "current_token_sid": self.user,
                "trusted_administration_sids": sorted(WINDOWS_ADMINS),
                "local_filesystem": "NTFS", "handles_closed": self.closed, "identity_lost": self.lost,
                "private_directory_candidate": str(self.created_directory) if self.created_directory is not None else None,
                "uninspected_objects": [{"path": str(entry["path"]), "handle": entry["handle"],
                                        "close_attempted": entry.get("close_attempted", False), "closed": entry["closed"]}
                                       for entry in self.objects if "facts" not in entry],
                "objects": [{"path": str(entry["path"]), "closed": entry["closed"],
                             "volume_and_file_id": entry["facts"]["identity"].hex(),
                             "owner_sid": entry["facts"]["security"]["owner"],
                             "dacl_sha256": hashlib.sha256(entry["facts"]["security"]["bytes"]).hexdigest(),
                             "dacl_protected": entry["facts"]["security"]["protected"],
                             "directory": entry["facts"]["directory"]}
                            for entry in self.objects if "facts" in entry]}

    def runtime(self, interpreter):
        root = Path(sys.base_prefix).absolute()
        require(sys.implementation.name == "cpython" and sys.platform == "win32" and
                sys.version_info[:3] == (3, 14, 6) and sys.prefix == sys.base_prefix and
                Path(interpreter).parent == root and Path(interpreter).name.lower() == "python.exe" and
                sysconfig.get_platform() == "win-amd64" and not sysconfig.get_config_var("Py_GIL_DISABLED") and
                not hasattr(sys, "gettotalrefcount"), "windows_runtime_layout")
        require(not any((root / name).exists() for name in ("pyvenv.cfg", "python314._pth", "python._pth")),
                "windows_runtime_layout")
        for directory in (root, root / "Lib", root / "DLLs"):
            self.directory(directory)
        for name in (root / "python314.dll", root / "python3.dll", root / "DLLs" / "_ssl.pyd",
                     root / "DLLs" / "_socket.pyd"):
            self.file(name)
        _, digest, _ = self.file(root / "Lib" / "subprocess.py")
        require(digest == WINDOWS_SUBPROCESS_SHA256, "windows_runtime_source")


def file_hash(path, end):
    if os.name == "nt":
        guard, failure, digest = None, None, None
        try:
            guard = WindowsTrust(end)
            digest = guard.file(path)[1]
        except BaseException as error:
            failure = error if isinstance(error, WorkerError) else WorkerError("windows_file_hash")
            guard = guard or getattr(error, "owner", None)
        if guard is not None:
            try:
                guard.close()
            except BaseException as error:
                if failure is None:
                    failure = error
                else:
                    failure.facts.setdefault("cleanup_errors", []).append(getattr(error, "code", "windows_trust_close"))
        if failure is not None:
            if not isinstance(failure, WorkerError):
                failure = WorkerError("windows_file_hash")
            failure.owner = guard
            failure.facts.update(windows_trust=guard.receipt() if guard is not None else {},
                                 trust_handles_closed=guard is not None and guard.closed)
            raise failure
        return digest
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
    global WINDOWS_ACTIVE_TRUST
    if os.name == "nt":
        try:
            guard = WindowsTrust(end)
        except WorkerError as error:
            WINDOWS_ACTIVE_TRUST = getattr(error, "owner", None)
            if WINDOWS_ACTIVE_TRUST is not None:
                WINDOWS_ACTIVE_TRUST.hard_end = header["hard_end"]
            raise
        WINDOWS_ACTIVE_TRUST = guard
        guard.hard_end = header["hard_end"]
        own, worker_hash, _ = guard.file(__file__)
        _, contract_hash, source = guard.file(own.with_name("request_contract.py"), collect=True)
        interpreter, interpreter_hash, _ = guard.file(sys.executable)
        require(worker_hash == header["worker_sha256"] and contract_hash == header["contract_sha256"] and
                os.path.normcase(str(interpreter)) == os.path.normcase(header["interpreter"]) and
                interpreter_hash == header["interpreter_sha256"], "trusted_identity")
        guard.runtime(interpreter)
        guard.directory(header["cwd"], private=True)
        guard.revalidate()
        module = types.ModuleType("request_contract")
        module.__file__ = str(own.with_name("request_contract.py"))
        sys.modules[module.__name__] = module
        exec(compile(source, module.__file__, "exec"), module.__dict__)
        guard.revalidate()
        return module
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
    if os.name != "nt":
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
        if WINDOWS_ACTIVE_TRUST is not None:
            WINDOWS_ACTIVE_TRUST.revalidate()
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
        if WINDOWS_ACTIVE_TRUST is not None:
            try:
                WINDOWS_ACTIVE_TRUST.close(cleanup_end=WINDOWS_ACTIVE_TRUST.hard_end)
                receipt["windows_trust_closed"] = WINDOWS_ACTIVE_TRUST.closed
            except BaseException as error:
                cleanup_error = getattr(error, "code", "windows_trust_close")
                receipt.setdefault("error", cleanup_error)
                receipt.setdefault("cleanup_errors", []).append(cleanup_error)
                receipt.update(status="failed",
                               windows_trust_closed=WINDOWS_ACTIVE_TRUST.closed)
                receipt.pop("answers", None)
                code = 1
            trust_receipt = getattr(WINDOWS_ACTIVE_TRUST, "receipt", None)
            if callable(trust_receipt):
                facts = trust_receipt()
                if type(facts) is dict:
                    receipt["windows_trust"] = {key: facts[key] for key in (
                        "handles_closed", "identity_lost", "private_directory_candidate") if key in facts}
                    receipt["native_resources"] = facts.get("native_resources", {})
        try:
            sys.stdin.close()
            receipt["credential_channel_closed"] = True
        except OSError:
            receipt.setdefault("error", "channel_close")
            receipt.setdefault("cleanup_errors", []).append("channel_close")
            receipt.update(status="failed")
            code = 1
    raw = json.dumps(receipt, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    if len(raw) > MAX_STDOUT:
        compact = {key: receipt[key] for key in ("credential_channel_closed", "network_attempted", "wire_sha256",
                   "socket_closed", "http_status", "header_bytes", "header_fields", "response_bytes",
                   "windows_trust_closed", "primary_error", "cleanup_errors", "native_resources") if key in receipt}
        compact["primary_error"] = receipt.get("error", "capture_size")
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
