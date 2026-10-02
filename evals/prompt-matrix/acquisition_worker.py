"""Standalone isolated registry reader; no executable hooks, subprocesses or threads."""

import base64
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import re
import socket
import ssl
import stat
import sys
import time
import urllib.error
import urllib.request
from urllib.parse import urljoin, urlsplit

MAX_ARCHIVE = 512 * 1024 * 1024
MAX_REQUEST = 16 * 1024
MAX_RECEIPT = 8 * 1024
CHUNK = 64 * 1024
MAX_REDIRECTS = 5
MAX_HEADER_LINE = 8192
MAX_HEADERS = 64
MAX_HEADER_BYTES = 32768
MAX_CA_BYTES = 4 * 1024 * 1024
MAX_CA_CERTIFICATES = 4096
CA_SOURCE = Path("/etc/ssl/cert.pem")
MAX_FAILURE_DEPTH = 3


class AcquisitionError(RuntimeError):
    pass


def require(value, reason):
    if not value:
        raise AcquisitionError(reason)


def remaining(end, clock=time.monotonic):
    value = end - clock()
    require(math.isfinite(end) and value > 0, "Acquisition operation deadline exceeded")
    return value


def registry_url(url):
    require(isinstance(url, str) and len(url) <= 4096, "Registry URL exceeds bound")
    parsed = urlsplit(url)
    require(parsed.scheme == "https" and parsed.hostname == "registry.npmjs.org" and
            parsed.port in (None, 443) and not parsed.username and not parsed.password and
            not parsed.fragment, "Registry URL escaped HTTPS confinement")
    return url


def metadata_input(metadata):
    url = registry_url(metadata["tarball"])
    integrity = metadata["integrity"]
    require(isinstance(integrity, str) and integrity.startswith("sha512-"), "Pinned SHA512 absent")
    try:
        digest = base64.b64decode(integrity[7:], validate=True)
    except (ValueError, TypeError) as error:
        raise AcquisitionError("Pinned SHA512 invalid") from error
    require(len(digest) == 64, "Pinned SHA512 length differs")
    sha256 = metadata.get("tarball_sha256")
    require(sha256 is None or isinstance(sha256, str) and re.fullmatch(r"[0-9a-f]{64}", sha256),
            "Pinned SHA256 invalid")
    return {"tarball": url, "integrity": integrity, "tarball_sha256": sha256}


def contained(root, path):
    root, path = Path(root).absolute(), Path(path).absolute()
    require(path != root and path.is_relative_to(root), "Acquisition path escaped fixture")
    require(not root.is_symlink() and root.resolve() == root, "Acquisition root is redirected")
    current = root
    for part in path.relative_to(root).parts:
        current = current / part
        require(not current.is_symlink(), "Acquisition path contains symlink")
    require(path.resolve().is_relative_to(root), "Acquisition path resolves outside fixture")
    return path


def bounded_json(path, limit):
    with Path(path).open("rb") as stream:
        raw = stream.read(limit + 1)
    require(len(raw) <= limit, "Acquisition JSON exceeds bound")
    return raw, json.loads(raw)


def exclusive_json(path, value):
    raw = json.dumps(value, sort_keys=True, separators=(",", ":")).encode() + b"\n"
    require(len(raw) <= MAX_RECEIPT, "Acquisition receipt exceeds bound")
    with exclusive_file(path) as output:
        require(output.write(raw) == len(raw), "Acquisition receipt write incomplete")


def exclusive_file(path):
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_BINARY", 0)
    descriptor = os.open(path, flags, 0o600)
    try:
        return os.fdopen(descriptor, "wb")
    except BaseException:
        os.close(descriptor)
        raise


def file_digest(path, end, clock=time.monotonic):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        while True:
            remaining(end, clock)
            chunk = stream.read(CHUNK)
            if not chunk:
                break
            digest.update(chunk)
    remaining(end, clock)
    return digest.hexdigest()


def stat_identity(value):
    return {"dev": value.st_dev, "ino": value.st_ino, "uid": value.st_uid, "gid": value.st_gid,
            "mode": value.st_mode, "bytes": value.st_size, "mtime_ns": value.st_mtime_ns}


def validate_ca_identity(value, source=False):
    require(isinstance(value, dict) and set(value) == {"dev", "ino", "uid", "gid", "mode", "bytes", "mtime_ns"}
            and all(type(item) is int and item >= 0 for item in value.values()), "Public CA identity invalid")
    require(stat.S_ISREG(value["mode"]) and not value["mode"] & 0o022 and
            0 < value["bytes"] <= MAX_CA_BYTES, "Public CA source is unsafe or empty")
    require(not source or value["uid"] == 0, "Public CA source is not root-owned")


def public_ca_bytes(raw):
    require(isinstance(raw, bytes) and 0 < len(raw) <= MAX_CA_BYTES, "Public CA bytes absent or oversized")
    beginnings = re.findall(br"-----BEGIN ([A-Z ]+)-----", raw)
    endings = re.findall(br"-----END ([A-Z ]+)-----", raw)
    require(0 < len(beginnings) <= MAX_CA_CERTIFICATES and beginnings == endings and
            all(kind == b"CERTIFICATE" for kind in beginnings), "Public CA PEM structure invalid")
    return hashlib.sha256(raw).hexdigest()


def public_ca_source():
    resolved = CA_SOURCE.resolve(strict=True)
    identity = stat_identity(resolved.stat())
    validate_ca_identity(identity, source=True)
    return resolved, identity


def bounded_ca_read(path, end, clock=time.monotonic):
    remaining(end, clock)
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_BINARY", 0)
    descriptor = os.open(path, flags)
    try:
        before = stat_identity(os.fstat(descriptor))
        validate_ca_identity(before)
        chunks, count = [], 0
        while True:
            remaining(end, clock)
            chunk = os.read(descriptor, min(CHUNK, MAX_CA_BYTES + 1 - count))
            remaining(end, clock)
            if not chunk:
                break
            count += len(chunk)
            require(count <= MAX_CA_BYTES, "Public CA byte limit exceeded")
            chunks.append(chunk)
        after = stat_identity(os.fstat(descriptor))
        require(before == after == stat_identity(Path(path).stat()), "Public CA changed during read")
        raw = b"".join(chunks)
        require(len(raw) == before["bytes"], "Public CA read incomplete")
        public_ca_bytes(raw)
        remaining(end, clock)
        return raw, before
    finally:
        os.close(descriptor)


def read_public_ca(end, clock=time.monotonic):
    resolved, identity = public_ca_source()
    remaining(end, clock)
    raw, actual = bounded_ca_read(resolved, end, clock)
    require(identity == actual and public_ca_source() == (resolved, identity), "Public CA source changed")
    return raw, resolved, identity


def validated_tls(request, end, clock=time.monotonic, platform=None):
    platform = sys.platform if platform is None else platform
    tls = request["tls"]
    require(isinstance(tls, dict) and tls.get("platform") == platform, "TLS platform binding differs")
    if platform in ("linux", "win32"):
        require(tls == {"platform": platform, "mode": "system_default"}, "TLS system mode differs")
        return None
    require(platform == "darwin" and set(tls) == {"platform", "mode", "source", "resolved_source", "source_identity",
            "snapshot", "snapshot_identity", "bytes", "sha256"} and tls["mode"] == "os_public_ca_snapshot" and
            tls["source"] == str(CA_SOURCE), "TLS public CA binding differs")
    validate_ca_identity(tls["source_identity"], source=True)
    validate_ca_identity(tls["snapshot_identity"])
    resolved, source = public_ca_source()
    require(str(resolved) == tls["resolved_source"] and source == tls["source_identity"], "Public CA source identity differs")
    snapshot = contained(request["fixture_root"], tls["snapshot"])
    require(snapshot == Path(request["receipt"]).parent / "public-ca.pem", "Public CA snapshot path differs")
    raw, identity = bounded_ca_read(snapshot, end, clock)
    require(identity == tls["snapshot_identity"] and type(tls["bytes"]) is int and len(raw) == tls["bytes"] ==
            tls["source_identity"]["bytes"] and public_ca_bytes(raw) == tls["sha256"], "Public CA snapshot identity/hash differs")
    remaining(end, clock)
    return snapshot


def tls_context(request, end, clock=time.monotonic, platform=None, factory=None):
    snapshot = validated_tls(request, end, clock, platform)
    factory = ssl.create_default_context if factory is None else factory
    context = factory(cafile=str(snapshot)) if snapshot is not None else factory()
    remaining(end, clock)
    require(context.verify_mode == ssl.CERT_REQUIRED and context.check_hostname is True,
            "TLS certificate/hostname verification weakened")
    require(getattr(context, "keylog_filename", None) is None, "TLS key logging is forbidden")
    if snapshot is not None:
        counts = context.cert_store_stats()
        require(type(counts.get("x509_ca")) is int and counts["x509_ca"] > 0, "Public CA context has no trust anchors")
        validated_tls(request, end, clock, platform)
    return context


def failure_facts(error, depth=0, seen=None):
    seen = set() if seen is None else seen
    if depth >= MAX_FAILURE_DEPTH or id(error) in seen:
        return {"type": "truncated", "category": "bound"}
    seen.add(id(error))
    if isinstance(error, urllib.error.HTTPError):
        facts = {"type": "HTTPError", "category": "http"}
        if type(error.code) is int and 100 <= error.code <= 599:
            facts["http_status"] = error.code
    elif isinstance(error, urllib.error.URLError):
        facts = {"type": "URLError", "category": "url"}
    elif isinstance(error, ssl.SSLCertVerificationError):
        facts = {"type": "SSLCertVerificationError", "category": "ssl_certificate"}
        code = getattr(error, "verify_code", None)
        if type(code) is int and 0 <= code <= 2147483647:
            facts["verify_code"] = code
    elif isinstance(error, ssl.SSLError):
        facts = {"type": "SSLError", "category": "ssl"}
    elif isinstance(error, OSError):
        kinds = (socket.gaierror, socket.herror, TimeoutError, ConnectionRefusedError, ConnectionResetError,
                 BrokenPipeError, FileNotFoundError, PermissionError)
        kind = next((value.__name__ for value in kinds if isinstance(error, value)), "OSError")
        facts = {"type": kind, "category": "os"}
    elif isinstance(error, AcquisitionError):
        facts = {"type": "AcquisitionError", "category": "fixture"}
    else:
        kind = next((kind for value, kind in ((None, "none"), (str, "str"), (int, "int"), (bool, "bool"), (bytes, "bytes"))
                     if (error is None if value is None else type(error) is value)), "Exception")
        facts = {"type": kind, "category": "other"}
    if isinstance(error, OSError) and type(error.errno) is int and -2147483648 <= error.errno <= 2147483647:
        facts["errno"] = error.errno
    if isinstance(error, urllib.error.URLError) and not isinstance(error, urllib.error.HTTPError):
        facts["reason"] = failure_facts(error.reason, depth + 1, seen)
    elif isinstance(error, BaseException):
        cause = error.__cause__ or error.__context__
        if cause is not None:
            facts["cause"] = failure_facts(cause, depth + 1, seen)
    return facts


def response_headers(response):
    headers = list(response.headers.items())
    require(len(headers) <= MAX_HEADERS and sum(len(k) + len(v) + 4 for k, v in headers) <= MAX_HEADER_BYTES,
            "Registry response headers exceed bound")
    require(all(len(k) + len(v) + 4 <= MAX_HEADER_LINE for k, v in headers), "Registry header line exceeds bound")
    lengths = response.headers.get_all("Content-Length", [])
    require(len(lengths) <= 1, "Ambiguous archive length")
    require(not lengths or re.fullmatch(r"[0-9]{1,10}", lengths[0]), "Invalid archive length")
    length = int(lengths[0]) if lengths else None
    require(length is None or length <= MAX_ARCHIVE, "Archive byte limit exceeded")
    require(not response.headers.get_all("Content-Encoding", []), "Encoded archive body unsupported")
    return length


class RegistryRedirect(urllib.request.HTTPRedirectHandler):
    def __init__(self, deadline, clock=time.monotonic):
        self.deadline, self.clock, self.hops = deadline, clock, []

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        remaining(self.deadline, self.clock)
        response_headers(type("Headers", (), {"headers": headers})())
        registry_url(newurl)
        require(len(self.hops) < MAX_REDIRECTS, "Registry redirect limit exceeded")
        self.hops.append(newurl)
        require(req is not None and req.get_method() == "GET", "Registry redirect method unsupported")
        return urllib.request.Request(newurl, method="GET")

    def http_error_302(self, req, fp, code, msg, headers):
        try:
            locations = headers.get_all("Location", [])
            require(len(locations) == 1 and len(locations[0]) <= 4096, "Registry redirect location ambiguous")
            request = self.redirect_request(req, fp, code, msg, headers, urljoin(req.full_url, locations[0]))
        finally:
            fp.close()
        return self.parent.open(request, timeout=min(10, remaining(self.deadline, self.clock)))

    http_error_301 = http_error_303 = http_error_307 = http_error_308 = http_error_302


def download(request, opener=None, redirect=None, clock=time.monotonic):
    end = request["operation_deadline"]
    remaining(end, clock)
    metadata = metadata_input(request["metadata"])
    root = Path(request["fixture_root"])
    archive = contained(root, request["archive"])
    require(not archive.exists(), "Archive destination already exists")
    if opener is None:
        http.client._MAXLINE = MAX_HEADER_LINE
        http.client._MAXHEADERS = MAX_HEADERS
        redirect = RegistryRedirect(end, clock)
        context = tls_context(request, end, clock)
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), redirect,
                                            urllib.request.HTTPSHandler(context=context))
    digest512, digest256, count = hashlib.sha512(), hashlib.sha256(), 0
    with opener.open(metadata["tarball"], timeout=min(10, remaining(end, clock))) as response:
        remaining(end, clock)
        final_url = registry_url(response.url)
        require(response.status == 200, "Registry response is not a complete archive")
        length = response_headers(response)
        with exclusive_file(archive) as output:
            while True:
                remaining(end, clock)
                chunk = response.read(CHUNK)
                remaining(end, clock)
                require(isinstance(chunk, bytes) and len(chunk) <= CHUNK, "Archive read exceeds bounded chunk")
                if not chunk:
                    break
                count += len(chunk)
                require(count <= MAX_ARCHIVE, "Archive byte limit exceeded")
                require(output.write(chunk) == len(chunk), "Archive write incomplete")
                digest512.update(chunk)
                digest256.update(chunk)
                remaining(end, clock)
        require(length is None or length == count, "Archive body ended before declared completion")
    remaining(end, clock)
    integrity = "sha512-" + base64.b64encode(digest512.digest()).decode()
    sha256 = digest256.hexdigest()
    require(integrity == metadata["integrity"], "Registry SHA512 integrity differs")
    require(metadata["tarball_sha256"] is None or sha256 == metadata["tarball_sha256"],
            "Reviewed archive SHA256 differs")
    return {"sha512": integrity, "sha256": sha256, "bytes": count, "source": metadata["tarball"],
            "final_url": final_url, "redirects": list(redirect.hops) if redirect else []}


def main():
    require(len(sys.argv) == 3 and sys.flags.isolated and sys.flags.no_site and sys.flags.dont_write_bytecode,
            "Helper requires isolated Python startup")
    request_path, request_hash = Path(sys.argv[1]).absolute(), sys.argv[2]
    base = (Path(__file__).resolve().parents[2] / "target-check/evals/prompt-matrix").resolve()
    contained(base, request_path)
    require(request_path.name == "request.json", "Helper request path differs")
    raw, request = bounded_json(request_path, MAX_REQUEST)
    require(isinstance(request, dict) and set(request) == {"version", "metadata", "fixture_root", "archive", "receipt",
            "operation_deadline", "worker_sha256", "interpreter", "interpreter_sha256", "tls"} and
            type(request["version"]) is int and request["version"] == 1, "Helper request schema differs")
    require(hashlib.sha256(raw).hexdigest() == request_hash, "Acquisition request changed")
    end = request["operation_deadline"]
    remaining(end)
    root = Path(request["fixture_root"])
    require(root.parent == base and re.fullmatch(r"[a-z][a-z0-9-]{2,39}", root.name), "Helper fixture root differs")
    contained(root, request_path)
    receipt_path = contained(root, request["receipt"])
    archive = contained(root, request["archive"])
    require(archive.parent == request_path.parent.parent and
            request_path.parent.name == archive.name + ".acquisition", "Acquisition archive/envelope join differs")
    require(request_path.parent == receipt_path.parent and request_path.name == "request.json" and
            receipt_path.name == "receipt.json", "Acquisition envelope paths differ")
    require(file_digest(__file__, end) == request["worker_sha256"] and
            Path(sys.executable).resolve() == Path(request["interpreter"]) and
            file_digest(sys.executable, end) == request["interpreter_sha256"], "Trusted acquisition inputs changed")
    try:
        receipt = download(request)
        remaining(end)
        exclusive_json(receipt_path, {"version": 1, "status": "completed", "request_sha256": request_hash,
                                      "archive": request["archive"], "result": receipt})
        remaining(end)
        return 0
    except Exception as error:
        failure = failure_facts(error)
        exclusive_json(receipt_path, {"version": 1, "status": "failed", "request_sha256": request_hash,
                                      "error_type": failure["type"], "failure": failure})
        return 1


if __name__ == "__main__":
    sys.exit(main())
